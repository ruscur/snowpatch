// Runner implementation for GitHub actions
// TODO initial support only targeting public GitHub
// TODO expecting jobs to spawn rather than manual triggers for now
use anyhow::{bail, Context, Error, Result};
use log::{debug, error, log_enabled, warn};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use serde::Deserialize;
use std::time::{Duration, Instant};
use ureq::{Agent, Response};
use url::Url;

use super::*;

#[derive(Clone)]
pub struct GitHubActions {
    agent: Agent,
    api: Url,
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
enum Conclusion {
    #[serde(rename = "action_required")]
    ActionRequired,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "failure")]
    Failure,
    #[serde(rename = "neutral")]
    Neutral,
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "skipped")]
    Skipped,
    #[serde(rename = "stale")]
    Stale,
    #[serde(rename = "timed_out")]
    TimedOut,
}
#[derive(Debug, Deserialize)]
enum Status {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "in_progress")]
    InProgress,
    #[serde(rename = "completed")]
    Completed,
}

// these APIs have an unreal amount of garbage in them...
#[derive(Debug, Deserialize)]
struct WorkflowRun {
    artifacts_url: Url,
    cancel_url: Url,
    html_url: Url, // not an API URL, for users
    conclusion: Option<Conclusion>,
    status: Status,
    name: String,
}

impl WorkflowRun {
    fn to_runner_result(&self) -> RunnerResult {
        RunnerResult {
            name: self.name.clone(),
            state: match self.status {
                Status::Queued => JobState::Waiting,
                Status::InProgress => JobState::Running,
                Status::Completed => {
                    match &self.conclusion {
                        Some(c) => {
                            match c {
                                Conclusion::ActionRequired => JobState::Failed,
                                Conclusion::Cancelled => JobState::Failed,
                                Conclusion::Failure => JobState::Completed,
                                Conclusion::Neutral => JobState::Completed,
                                Conclusion::Success => JobState::Completed,
                                Conclusion::Skipped => JobState::Failed, // XXX
                                Conclusion::Stale => JobState::Failed,
                                Conclusion::TimedOut => JobState::Failed,
                            }
                        }
                        None => JobState::Failed,
                    }
                }
            },
            outcome: match &self.conclusion {
                Some(c) => {
                    match c {
                        Conclusion::ActionRequired => Some(TestState::Warning), // XXX
                        Conclusion::Cancelled => Some(TestState::Fail),
                        Conclusion::Failure => Some(TestState::Fail),
                        Conclusion::Neutral => Some(TestState::Warning),
                        Conclusion::Success => Some(TestState::Success),
                        Conclusion::Skipped => Some(TestState::Warning),
                        Conclusion::Stale => Some(TestState::Warning),
                        Conclusion::TimedOut => Some(TestState::Fail),
                    }
                }
                None => None,
            },
            url: Some(self.html_url.clone()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct WorkflowRuns {
    #[serde(rename = "total_count")]
    count: u64,
    #[serde(rename = "workflow_runs")]
    runs: Vec<WorkflowRun>,
}

impl GitHubActions {
    pub fn new(agent: Agent, url: &Url, token: Option<String>) -> Result<GitHubActions> {
        // Need to find the owner and repo from the URL
        let mut segments = url
            .path_segments()
            .context("GitHub URL needs full path to repo")?;

        let owner = segments
            .next()
            .context("GitHub URL needs full path to repo")?;
        let repo = segments
            .next()
            .context("GitHub URL needs full path to repo")?;

        if segments.next().is_some() {
            bail!("GitHub URL should not contain anything other than the repo path");
        }

        let mut api_url = Url::parse("https://api.github.com/")?;
        api_url
            .path_segments_mut()
            .map_err(|_| Error::msg("URL is boned"))? // URL crate sucks
            .push("repos")
            .push(owner)
            .push(repo);

        let gha = GitHubActions {
            agent,
            api: api_url.clone(),
            token,
        };

        // Smoke test to check the API URL works
        gha.api_req("GET", &api_url)
            .context(format!("Couldn't find branch with URL {}", &api_url))?;

        Ok(gha)
    }

    fn api_req(&self, method: &str, url: &Url) -> Result<Response> {
        let mut resp = self
            .agent
            .request_url(method, &url)
            .set("Accept", "application/vnd.github.v3+json");

        if let Some(t) = &self.token {
            resp = resp.set("Authorization", &format!("token {}", t));
        }

        let resp = resp.call()?;

        Ok(resp)
    }

    fn build_branch_url(&self, branch: &str) -> Result<Url> {
        let mut branch_url = self.api.clone();
        branch_url
            .path_segments_mut()
            .map_err(|_| Error::msg("URL is boned"))? // URL crate sucks
            .push("branches")
            .push(branch);

        Ok(branch_url)
    }

    fn get_workflow_runs(&self, branch: &str) -> Result<WorkflowRuns> {
        let mut runs_url = self.api.clone();
        runs_url
            .path_segments_mut()
            .map_err(|_| Error::msg("URL is boned"))? // URL crate sucks
            .push("actions")
            .push("runs");
        runs_url.query_pairs_mut().append_pair("branch", branch);

        Ok(serde_json::from_value(
            self.api_req("GET", &runs_url)?.into_json()?,
        )?)
    }
}

impl Runner for GitHubActions {
    fn get_handle(&self) -> String {
        "github".to_string()
    }

    fn start_work(&self, branch_name: &String, _url: Option<&Url>) -> Result<()> {
        // TODO no handling of different triggers
        let trigger_on_push = true;

        if trigger_on_push {
            // we just need to check that something is happening
            let timeout = Duration::from_secs(600);
            let start = Instant::now();
            let mut wfr = self.get_workflow_runs(&branch_name)?;
            while Instant::now().duration_since(start) < timeout {
                if wfr.runs.is_empty() {
                    warn!("Branch {} has no workflows started!", branch_name);
                } else {
                    break;
                }
                thread::sleep(Duration::from_secs(30));
                wfr = self.get_workflow_runs(&branch_name)?;
            }

            // TODO no handling of timeout failure case
            if log_enabled!(log::Level::Debug) {
                wfr.runs.iter().for_each(|run| {
                    debug!(
                        "Branch {} with workflow {} has status {:?} and conclusion {:?}",
                        branch_name, run.name, run.status, run.conclusion
                    );
                });
            }
        } else {
            todo!();
        }

        Ok(())
    }

    fn get_progress(&self, branch_name: &String, _url: Option<&Url>) -> Result<Vec<RunnerResult>> {
        let wfr = self.get_workflow_runs(&branch_name)?;

        let progress: Vec<RunnerResult> = wfr
            .runs
            .par_iter()
            .map(|run| run.to_runner_result())
            .collect();

        Ok(progress)
    }

    fn clean_up(&self, _branch_name: &String, _url: Option<&Url>) -> Result<()> {
        todo!()
    }
}

mod tests {
    use super::*;
    use anyhow::Result;
    use ureq::Agent;

    fn get_gha() -> GitHubActions {
        GitHubActions::new(
            Agent::new(),
            &Url::parse("https://github.com/ruscur/linux-ci").unwrap(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn get_actions() -> Result<()> {
        let gha = get_gha();

        let wfr: WorkflowRuns = gha.get_workflow_runs("snowpatch/254076")?;

        println!("Found {} workflow runs.", wfr.count);

        wfr.runs.iter().for_each(|run| {
            println!(
                "Workflow {} has status {:?} and conclusion {:?} as {:?}",
                run.name,
                run.status,
                run.conclusion,
                run.to_runner_result()
            );
        });

        Ok(())
    }

    // TODO: migrate to runner.rs
    #[test]
    fn get_progress() -> Result<()> {
        let runner: Box<dyn Runner> = Box::new(get_gha());
        let jobs: Vec<RunnerResult> = runner.get_progress(&"snowpatch/254076".to_string(), None)?;

        let finished_jobs: Vec<&RunnerResult> = jobs
            .par_iter()
            .filter(|j| j.state != JobState::Waiting)
            .filter(|j| j.state != JobState::Running)
            .collect();

        println!("{:?}", jobs);
        println!("{:?}", finished_jobs);

        Ok(())
    }
}
