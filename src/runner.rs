// Attempt to define an API that runners have to implement
use crate::config::{Runner as RunnerConfig, Trigger};
use crate::database::{move_to_new_queue, wait_for_tree};
use crate::patchwork::TestState;
use crate::DB;
use anyhow::{bail, Result};
use dyn_clone::DynClone;
use github::GitHubActions;
use log::{debug, error, trace};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use sled::IVec;
use std::thread;
use std::time::Duration;
use ureq::Agent;
use url::Url;

pub mod github;

pub trait Runner: DynClone {
    fn get_handle(&self) -> String;
    /// Set up any necessary state, and manually kick off tests if necessary.
    fn start_work(&self, branch_name: &String, url: Option<&Url>) -> Result<()>;
    /// For each spawned test, return status.  If tests are finished, should be enough to report.
    fn get_progress(&self, branch_name: &String, url: Option<&Url>) -> Result<Vec<RunnerResult>>;
    /// Assume this can be run at any time (i.e. other fatal failures) to clean up all state, local & remote.
    fn clean_up(&self, branch_name: &String, url: Option<&Url>) -> Result<()>;
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
pub enum JobState {
    Waiting,   // has not begun executing
    Running,   // is currently executing
    Completed, // has completed executing, result does not matter
    Failed,    // couldn't complete for non-code related reasons
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RunnerResult {
    pub name: String, // name of the running job
    pub state: JobState,
    pub outcome: TestState,
    pub url: Option<Url>, // user-facing results URL
    pub description: Option<String>,
}

// Exceedingly cursed type signature
pub fn init(config: Vec<RunnerConfig>, agent: Agent) -> Result<Vec<Box<dyn Runner + Send>>> {
    let mut runners: Vec<Box<dyn Runner + Send>> = vec![];
    let tree = DB.open_tree(b"remotes to push to")?;
    for runner in config {
        match runner {
            RunnerConfig::GitHub {
                trigger,
                url,
                token,
            } => match trigger {
                Trigger::OnPush { remote } => {
                    tree.insert(remote.as_bytes(), b"github")?;
                    let gha = GitHubActions::new(agent.clone(), &url, token)?;
                    runners.push(Box::new(gha));
                }

                Trigger::Manual { data: _ } => todo!(),
            },
        }
    }

    Ok(runners)
}

// Should never return
pub fn new_job_watcher(runner: Box<dyn Runner + Send>) -> Result<()> {
    let handle = runner.get_handle();
    let inbound = DB.open_tree(format!("{} queue", handle))?;
    let outbound = DB.open_tree(format!("{} working", handle))?;

    loop {
        for result in inbound.iter() {
            let (key, _value) = result?;
            let local_branch_name = String::from_utf8_lossy(&key).to_string();
            let remote_branch_name = format!("snowpatch/{}", local_branch_name);
            trace!(
                "started new_job_watcher() local {} remote {}",
                local_branch_name,
                remote_branch_name
            );

            runner.start_work(&remote_branch_name, None)?;
            move_to_new_queue(&inbound, &outbound, &key)?;
            trace!("new_job_watcher() started work & moved queue");

            // time to spawn stuff :)
            let runner = dyn_clone::clone_box(&*runner);
            rayon::spawn(move || {
                wait_for_completion(runner, &local_branch_name, &remote_branch_name, None).unwrap();
            });
            trace!("new_job_watcher() spawned watcher, next...");
        }

        trace!("new_job_watcher() calling wait_for_tree()...");
        wait_for_tree(&inbound);
    }
}

fn wait_for_completion(
    runner: Box<dyn Runner>,
    local_branch_name: &String,
    remote_branch_name: &String,
    url: Option<&Url>,
) -> Result<()> {
    let handle = runner.get_handle();
    let inbound = DB.open_tree(format!("{} working", handle))?;
    let outbound = DB.open_tree(format!("needs dispatch"))?;
    let poll_interval = Duration::from_secs(90); // TODO configurable
    let mut completed_jobs: Vec<String> = vec![];

    loop {
        let jobs: Vec<RunnerResult> = runner.get_progress(remote_branch_name, url)?;

        let finished_jobs: Vec<&RunnerResult> = jobs
            .par_iter()
            .filter(|j| j.state != JobState::Waiting)
            .filter(|j| j.state != JobState::Running)
            .collect();

        // TODO jobs that failed to properly run are just silently dead
        for j in &finished_jobs {
            if !completed_jobs.contains(&j.name) {
                let key = format!("{} {} {}", handle, local_branch_name, j.name);
                debug!("Found finished job, sending to dispatch: {}", key);
                outbound.insert(&key.as_bytes(), bincode::serialize(&j)?)?;
                completed_jobs.push(j.name.clone())
            }
        }

        debug!(
            "{} has {} jobs done of {} total",
            remote_branch_name,
            finished_jobs.len(),
            jobs.len()
        );
        if jobs.len() == finished_jobs.len() {
            break;
        } else {
            thread::sleep(poll_interval);
        }
    }

    inbound.remove(local_branch_name)?;

    Ok(())
}
