use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use bincode::{deserialize, serialize};
use git2::build::CheckoutBuilder;
use git2::Commit;
use git2::Cred;
use git2::Diff;
use git2::ObjectType;
use git2::PushOptions;
use git2::RemoteCallbacks;
use git2::Repository;
use git2::Worktree;
use git2::WorktreePruneOptions;
use log::debug;
use log::error;
use log::info;
use log::warn;
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use std::time::Instant;
use url::Url;

use crate::database::*;
use crate::patchwork::*;
use crate::DB;

pub struct GitOps {
    repo: Repository,
    pool: ThreadPool,
    workdir: PathBuf,
}

impl GitOps {
    pub fn new(repo_dir: String, worker_count: usize, work_dir: String) -> Result<GitOps> {
        let repo = Repository::open(repo_dir)?;
        let pool = ThreadPoolBuilder::new()
            .num_threads(worker_count)
            .thread_name(|i| format!("snowpatch{}", i))
            .build()?;
        let workdir = PathBuf::from(work_dir);

        Ok(GitOps {
            repo,
            pool,
            workdir,
        })
    }

    /// git2::Worktree:validate() is a sham.
    /// Tests include: removing .git, deleting directory entirely
    /// `git worktree list` picks this up but git2 doesn't.
    /// As a better validation, try opening it as a Repository.
    fn validate_worktree(&self, wt: &Worktree) -> Result<()> {
        wt.validate()?;
        Repository::open_from_worktree(wt)?;

        Ok(())
    }

    fn get_worktree_checkout_path(&self, name: &str) -> PathBuf {
        let mut path = self.workdir.clone();
        path.push(name);

        path
    }

    fn get_worktree_dotgit_path(&self, name: &str) -> PathBuf {
        let mut path = self.repo.path().to_path_buf();
        path.push("worktrees");
        path.push(name);

        path
    }

    fn create_worktree(&self, name: &str, wt: Option<Worktree>) -> Result<Worktree> {
        let checkout_path = self.get_worktree_checkout_path(name);
        let dotgit_path = self.get_worktree_dotgit_path(name);

        match wt {
            Some(wt) => {
                let mut wpo = WorktreePruneOptions::new();
                wpo.valid(true);
                wpo.locked(true);
                wpo.working_tree(true);
                // This should nuke everything except the branch.
                let _ = wt.prune(Some(&mut wpo))?;
            }
            None => {}
        }

        if checkout_path.exists() {
            fs::remove_dir_all(&checkout_path)?
        }
        if dotgit_path.exists() {
            fs::remove_dir_all(&dotgit_path)?
        }

        match self.repo.find_branch(name, git2::BranchType::Local) {
            Ok(mut branch) => branch.delete()?,
            Err(_) => {}
        };

        let wt = self.repo.worktree(name, &checkout_path, None)?;

        Ok(wt)
    }
    /// Creates worktrees in `workdir` with names from `snowpatch0` up to
    /// the worker count.
    pub fn init_worktrees(&self) -> Result<()> {
        let worktree_names: Vec<String> = (0..self.pool.current_num_threads())
            .map(|i| format!("snowpatch{}", i))
            .collect();

        // operations on the base repo *cannot* be done in parallel.
        let worktrees: Result<Vec<Worktree>> = worktree_names
            .iter()
            .map(|name| match self.repo.find_worktree(name) {
                Ok(wt) => {
                    debug!("Found existing worktree {}", name);
                    match self.validate_worktree(&wt) {
                        Ok(_) => Ok(wt),
                        Err(e) => {
                            warn!("Worktree {} failed validation, recreating.", name);
                            dbg!("Validation error {}", e.to_string());
                            self.create_worktree(name, Some(wt))
                        }
                    }
                }
                Err(_) => {
                    info!("Creating new worktree {}", name);
                    self.create_worktree(name, None)
                }
            })
            .collect();

        let _ = worktrees?;

        dbg!(worktree_names);

        Ok(())
    }

    /// This function does not return unless there's an error.
    /// Watch for stuff on the "needs testing" tree and handle it.
    pub fn ingest(&self) -> Result<()> {
        let inbound = DB.open_tree(b"needs testing")?;
        let outbound = DB.open_tree(b"awaiting git worker")?;

        loop {
            for result in inbound.iter() {
                let (key, _value) = result?;
                let series_id: u64 = deserialize(&key)?;

                move_to_new_queue(&inbound, &outbound, &key)?;
                let workdir = self.workdir.clone();

                self.pool.spawn(move || {
                    try_do_work(series_id, workdir)
                        .unwrap_or_else(|e| error!("Boned: {}", e.to_string()))
                });
            }

            let sub = inbound.watch_prefix(vec![]);
            // blocks until there's an update to the tree
            for _ in sub.take(1) {}
            // maybe we have data to care about now.
        }
    }
}

fn try_do_work(id: u64, workdir: PathBuf) -> Result<()> {
    let key = serialize(&id)?;
    let worker_id = rayon::current_thread_index().unwrap();
    let inbound = DB.open_tree(b"awaiting git worker")?;
    let outbound = DB.open_tree(format!("git worker {}", worker_id))?;
    let failed = DB.open_tree(b"git failures")?;

    debug!("I am worker {} with patch {}.", worker_id, id);

    // This should never happen.
    while outbound.iter().count() > 0 {
        debug!("I am worker {} with patch {} waiting.", worker_id, id);
        wait_for_tree(&outbound);
    }

    move_to_new_queue(&inbound, &outbound, &key)?;

    let result = do_work(id, workdir);

    match result {
        Ok(_) => {
            info!("Git ops on series {} succeded!", id);
            outbound.remove(&key)?;
            Ok(())
        }
        Err(e) => {
            info!("Git ops on series {} failed: {}", id, e.to_string());
            move_to_new_queue(&outbound, &failed, &key)?;
            Ok(())
        }
    }
}

fn test_apply(repo: &Repository, diff: &Diff) -> Result<()> {
    let mut ao = git2::ApplyOptions::new();
    ao.check(true);
    let result = repo.apply(&diff, git2::ApplyLocation::Both, Some(&mut ao));

    Ok(result?)
}

/// This should restore
fn clean_and_reset(repo: &Repository, branch_name: &str) -> Result<()> {
    // There's no git2 implementation of git clean.
    // Hopefully resetting is enough since we check all applications before
    // actually going through with them.
    let branch = repo.find_branch(branch_name, git2::BranchType::Local)?;
    let target = branch
        .get()
        .resolve()?
        .peel(ObjectType::Commit)?
        .into_commit();

    // Can't bubble up this error because it includes repo references
    // which don't implement Sync.
    let target = match target {
        Ok(c) => c,
        Err(_) => bail!("Couldn't get commit from {}", branch_name),
    };

    let mut cb = CheckoutBuilder::new();
    cb.force();
    cb.remove_untracked(true);
    cb.use_theirs(true);

    repo.reset(target.as_object(), git2::ResetType::Hard, Some(&mut cb))?;
    Ok(())
}

fn get_git_push_options() -> Result<PushOptions<'static>> {
    let mut callbacks = RemoteCallbacks::new();

    callbacks.credentials(|_url, username, _allowed_types| {
        let username = username.unwrap_or("git");
        Cred::ssh_key(username, None, Path::new("/home/ruscur/.ssh/id_rsa"), None)
    });

    let mut po = PushOptions::new();
    po.remote_callbacks(callbacks);

    Ok(po)
}

fn do_work(id: u64, workdir: PathBuf) -> Result<()> {
    let key = serialize(&id)?;
    let worker_id = rayon::current_thread_index().unwrap();
    let my_tree = DB.open_tree(format!("git worker {}", worker_id))?;

    // We (hopefully) now have exclusive access to the worktree.
    let mut worktree_path = workdir.clone();
    worktree_path.push(format!("snowpatch{}", worker_id));
    let repo = Repository::open(worktree_path)?;

    clean_and_reset(&repo, "master")?;

    let mbox_url: String = deserialize(&my_tree.get(&key)?.unwrap())?;
    let mbox_url = Url::parse(&mbox_url)?;

    let mbox = download_file(&mbox_url)?;

    let diff = Diff::from_buffer(&mbox)?;

    let deltacount = diff.deltas().count();
    println!(
        "I'm thread {} with series {} with {} deltas",
        worker_id, id, deltacount
    );

    // By testing first there's no consequences on the tree if it fails
    test_apply(&repo, &diff)?;

    repo.apply(&diff, git2::ApplyLocation::Both, None)?;
    let sig = repo.signature()?;
    let msg = format!("From patchwork series {}", &id);
    let head_commit = repo
        .head()?
        .resolve()?
        .peel(ObjectType::Commit)?
        .into_commit();

    // Can't bubble up this error because it includes repo references
    // which don't implement Sync.
    let head_commit = match head_commit {
        Ok(c) => c,
        Err(_) => bail!("Couldn't get HEAD commit on {}", &id),
    };

    // git2 really sucks.
    let _commit_id = repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        &msg,
        &repo.find_tree(repo.index()?.write_tree()?)?,
        &[&head_commit],
    )?;

    // TODO this needs to come from the DB from the config.
    let mut remote = repo.find_remote("gitlab2")?;

    remote.push(
        &[format!("+HEAD:refs/heads/snowpatch/{}", &id).as_str()],
        Some(&mut get_git_push_options()?),
    )?;

    Ok(())
}
