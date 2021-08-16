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
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
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
        // Create the work directory in case it doesn't already exist
        fs::create_dir_all(&workdir)?;

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

            // wait until there's more stuff to do
            wait_for_tree(&inbound);
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

// Yeah, this function sucks.  There's a lot of type and borrow checker fighting here.
fn get_git_push_options() -> Result<PushOptions<'static>> {
    let mut callbacks = RemoteCallbacks::new();

    let public_key_path = String::from_utf8_lossy(
        &DB.get(b"ssh public key path")?
            .context("Couldn't find SSH public key path in database")?,
    )
    .to_string();
    let private_key_path = String::from_utf8_lossy(
        &DB.get(b"ssh private key path")?
            .context("Couldn't find SSH private key path in database")?,
    )
    .to_string();

    callbacks.credentials(move |_url, username, _allowed_types| {
        let public_key_path = public_key_path.clone();
        let private_key_path = private_key_path.clone();

        Cred::ssh_key(
            username.unwrap_or("git"),
            Some(&PathBuf::from(public_key_path)),
            &PathBuf::from(private_key_path),
            None,
        )
    });

    let mut po = PushOptions::new();
    po.remote_callbacks(callbacks);

    Ok(po)
}

/// The nuclear option.
fn git_binary_apply_mbox(workdir: &Path, mbox: &Vec<u8>) -> Result<()> {
    let mut git_process = Command::new("git")
        .arg("apply")
        .arg("--check")
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Couldn't invoke git binary");

    let mut stdin = git_process.stdin.take().expect("Couldn't open stdin");
    stdin.write_all(mbox).expect("Couldn't write to stdin");
    drop(stdin);
    let output = git_process
        .wait_with_output()
        .expect("Couldn't read stdout");

    if !output.status.success() {
        debug!("{}", String::from_utf8_lossy(&output.stdout));
        bail!("Series doesn't apply with git binary either.")
    }

    // Now we do it again, with meaning.
    let mut git_process = Command::new("git")
        .arg("apply")
        .arg("--index")
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Couldn't invoke git binary");

    let mut stdin = git_process.stdin.take().expect("Couldn't open stdin");
    stdin.write_all(mbox).expect("Couldn't write to stdin");
    drop(stdin);
    let output = git_process
        .wait_with_output()
        .expect("Couldn't read stdout");

    if !output.status.success() {
        debug!("{}", String::from_utf8_lossy(&output.stdout));
        bail!("git decided to screw us after we did --check, nice")
    }

    Ok(())
}

fn do_work(id: u64, workdir: PathBuf) -> Result<()> {
    let key = serialize(&id)?;
    let worker_id = rayon::current_thread_index().unwrap();
    let my_tree = DB.open_tree(format!("git worker {}", worker_id))?;

    // We (hopefully) now have exclusive access to the worktree.
    let mut worktree_path = workdir.clone();
    worktree_path.push(format!("snowpatch{}", worker_id));
    let repo = Repository::open(&worktree_path)?;

    clean_and_reset(&repo, "master")?;

    let mbox_url: String = deserialize(&my_tree.get(&key)?.unwrap())?;
    let mbox_url = Url::parse(&mbox_url)?;

    let mbox = download_file(&mbox_url)?;

    let diff = Diff::from_buffer(&mbox)?;

    let deltacount = diff.deltas().count();
    debug!(
        "I'm thread {} with series {} with {} deltas",
        worker_id, id, deltacount
    );

    // By testing first there's no consequences on the tree if it fails
    match test_apply(&repo, &diff) {
        Ok(_) => repo.apply(&diff, git2::ApplyLocation::Both, None)?,
        Err(_) => {
            // There's a disappointingly large chance that a series that won't
            // apply with git2 will actually apply just fine with the binary.
            // So let's do that instead.
            git_binary_apply_mbox(&worktree_path, &mbox)?;
        }
    }

    let sig = repo.signature()?;

    let url_prefix: String = String::from_utf8_lossy(
        &DB.get("patchwork series link prefix")?
            .context("Couldn't find Patchwork series link prefix in database")?,
    )
    .to_string();
    let msg = format!("From patchwork series {0}\n\n{1}{0}", &id, url_prefix);
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

    let remote_list_tree = DB.open_tree(b"remotes to push to")?;
    let runners = db_collect_string_values(remote_list_tree.iter())?;

    // only do this for OnPush? otherwise update different tree
    for (remote, runner) in runners {
        let mut remote: git2::Remote = repo.find_remote(&remote)?;

        // XXX
        let push_result = remote.push(
            &[format!("HEAD:refs/heads/snowpatch/{}", &id).as_str()],
            Some(&mut get_git_push_options()?),
        );

        match push_result {
            Ok(_) => (),
            Err(e) => {
                if e.code() == git2::ErrorCode::NotFastForward {
                    // this is the easiest way to figure out the branch already exists.
                    // if we push again, all the jobs will trigger again.
                    // this creates a lot of duplicate work.
                    // for now, just let it happen, cap'n.
                    warn!("Remote branch already existed, letting it fly...");
                } else {
                    bail!("Couldn't push: {}", e.to_string());
                }
            }
        }

        let runner_queue_tree = DB.open_tree(format!("{} queue", runner))?;
        runner_queue_tree.insert(&id.to_string(), b"new")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::{self, Read};

    #[test]
    fn test_git_binary() {
        let mut mbox = File::open("/home/ruscur/patch.mbox").unwrap();
        let mut buffer: Vec<u8> = vec![];
        mbox.read_to_end(&mut buffer).unwrap();
        git_binary_apply_mbox(&PathBuf::from("/home/ruscur/code/linux"), &buffer).unwrap();
    }
}
