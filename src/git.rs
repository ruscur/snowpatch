use anyhow::Result;
use bincode::{deserialize, serialize};
use git2::Repository;
use git2::Worktree;
use git2::WorktreePruneOptions;
use log::debug;
use log::info;
use log::warn;
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::fs;
use std::path::{Path, PathBuf};

use crate::database::*;
use crate::DB;

pub struct GitOps {
    repo: Repository,
    pool: ThreadPool,
    workdir: PathBuf,
}

fn do_work(id: u64) {
    println!(
        "I'm supposed to be doing work on id {} as thread {}!",
        id,
        rayon::current_thread_index().unwrap()
    );
}

impl GitOps {
    pub fn new(repo_dir: String, worker_count: usize, work_dir: String) -> Result<GitOps> {
        let repo = Repository::open(repo_dir)?;
        let pool = ThreadPoolBuilder::new().num_threads(worker_count).build()?;
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
        let outbound = DB.open_tree(b"git ops in progress")?;

        println!(
            "LIVING KEKW as thread id {}",
            rayon::current_thread_index().unwrap()
        );

        loop {
            for result in inbound.iter() {
                let (key, _value) = result?;
                let series_id: u64 = deserialize(&key)?;

                move_to_new_queue(&inbound, &outbound, &key)?;

                self.pool.spawn(move || do_work(series_id));
            }

            let sub = inbound.watch_prefix(vec![]);
            // blocks until there's an update to the tree
            for _ in sub.take(1) {}
            // maybe we have data to care about now.
        }
    }
}
