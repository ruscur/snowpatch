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

#[derive(Debug, Deserialize, PartialEq)]
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
    #[serde(rename = "startup_failure")]
    StartupFailure,
    #[serde(rename = "timed_out")]
    TimedOut,
}
#[derive(Debug, Deserialize, PartialEq)]
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
    jobs_url: Url,
    conclusion: Option<Conclusion>,
    status: Status,
    name: String,
}

#[derive(Debug, Deserialize)]
struct WorkflowRuns {
    #[serde(rename = "total_count")]
    count: u64,
    #[serde(rename = "workflow_runs")]
    runs: Vec<WorkflowRun>,
}

#[derive(Debug, Deserialize)]
struct CheckRunOutput {
    annotations_count: u64,
    annotations_url: Url,
    title: String,
}

#[derive(Debug, Deserialize)]
struct CheckRun {
    output: CheckRunOutput,
}

#[derive(Debug, Deserialize)]
struct Step {
    conclusion: Conclusion,
    name: String,
    number: u64,
    status: Status,
}

#[derive(Debug, Deserialize)]
struct Job {
    name: String,
    steps: Vec<Step>,
    url: Url,
    html_url: Url,
    check_run_url: Url,
    conclusion: Conclusion,
}

#[derive(Debug, Deserialize)]
struct Jobs {
    jobs: Vec<Job>,
    #[serde(rename = "total_count")]
    count: u64,
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

    /// Get a WorkflowRun from its API URL.
    fn get_workflow_run(&self, run_url: &Url) -> Result<WorkflowRun> {
        Ok(serde_json::from_value(
            self.api_req("GET", &run_url)?.into_json()?,
        )?)
    }

    /// Get a CheckRun from its API URL.
    fn get_check_run(&self, run_url: &Url) -> Result<CheckRun> {
        Ok(serde_json::from_value(
            self.api_req("GET", &run_url)?.into_json()?,
        )?)
    }

    fn get_workflow_runs_for_branch(&self, branch: &str) -> Result<WorkflowRuns> {
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

    fn get_jobs(&self, jobs_url: &Url) -> Result<Jobs> {
        Ok(serde_json::from_value(
            self.api_req("GET", &jobs_url)?.into_json()?,
        )?)
    }

    fn wfr_to_runner_result(&self, wfr: &WorkflowRun) -> Result<RunnerResult> {
        let state = match wfr.status {
            Status::Queued => JobState::Waiting,
            Status::InProgress => JobState::Running,
            Status::Completed => match &wfr.conclusion {
                Some(c) => match c {
                    Conclusion::ActionRequired => JobState::Failed,
                    Conclusion::Cancelled => JobState::Failed,
                    Conclusion::Failure => JobState::Completed,
                    Conclusion::Neutral => JobState::Completed,
                    Conclusion::Success => JobState::Completed,
                    Conclusion::Skipped => JobState::Failed,
                    Conclusion::Stale => JobState::Failed,
                    Conclusion::TimedOut => JobState::Failed,
                    Conclusion::StartupFailure => JobState::Failed,
                },
                None => JobState::Failed,
            },
        };

        let (outcome, description): (TestState, String) = match &wfr.conclusion {
            Some(c) => match c {
                Conclusion::ActionRequired => (
                    TestState::Fail,
                    String::from("Manual intervention required"),
                ),
                Conclusion::Cancelled => (TestState::Fail, String::from("Job manually cancelled")),
                Conclusion::Failure => {
                    let jobs = self.get_jobs(&wfr.jobs_url)?;
                    let failures: Vec<&Job> = jobs
                        .jobs
                        .par_iter()
                        .filter(|j| j.conclusion == Conclusion::Failure)
                        .collect();
                    let failure_count = failures.len();

                    let description: String = if failure_count == 0 {
                        error!("Run reports Failure but jobs have no failures?");
                        debug!("{:?}", jobs);
                        bail!("Run reported Failure but couldn't find failures in jobs");
                    } else if failure_count > 1 {
                        format!("{} of {} jobs failed.", failure_count, jobs.count)
                    } else {
                        let failed_job = failures.first().context("Rust can't count.")?;
                        let failed_steps: Vec<&Step> = failed_job
                            .steps
                            .iter()
                            .filter(|s| s.conclusion == Conclusion::Failure)
                            .collect();

                        for step in &failed_steps {
                            if step.name == "Set up job" {
                                (
                                    TestState::Warning,
                                    format!(
                                        "{} failed to run, not the patch's fault",
                                        failed_job.name
                                    ),
                                );
                            }
                        }
                        if failed_steps.len() == 1 {
                            let step = failed_steps.first().unwrap();
                            format!("{} failed at step {}.", failed_job.name, step.name)
                        } else {
                            format!(
                                "{} failed {} of {} steps.",
                                failed_job.name,
                                failed_steps.len(),
                                failed_job.steps.len()
                            )
                        }
                    };
                    (TestState::Fail, description)
                }
                Conclusion::Neutral => (
                    TestState::Warning,
                    String::from("Neutral job result, check for details"),
                ),
                Conclusion::Success => {
                    // Glad it worked, now let's see if there's any warnings.
                    let jobs = self.get_jobs(&wfr.jobs_url)?;
                    let check_runs: Vec<CheckRunOutput> = jobs
                        .jobs
                        .par_iter()
                        .map(|j| self.get_check_run(&j.check_run_url))
                        .filter(|cr| cr.is_ok()) // XXX how to do nicely? and_then()?
                        .map(|cr| cr.unwrap().output)
                        .filter(|cr| cr.annotations_count > 0)
                        .collect();

                    if check_runs.len() == 0 {
                        (
                            TestState::Success,
                            format!("Successfully ran {} jobs.", jobs.count),
                        )
                    } else if check_runs.len() == 1 {
                        let check_run = check_runs.first().unwrap(); // safe because len
                        (
                            TestState::Warning,
                            format!(
                                "{} found {} issues.",
                                check_run.title, check_run.annotations_count
                            ),
                        )
                    } else {
                        let total_annotations: u64 =
                            check_runs.par_iter().map(|cr| cr.annotations_count).sum();
                        (
                            TestState::Warning,
                            format!(
                                "Found {} issues from {} of {} jobs.",
                                &total_annotations,
                                check_runs.len(),
                                jobs.count
                            ),
                        )
                    }
                }
                Conclusion::Skipped => (TestState::Warning, String::from("Job skipped.")),
                Conclusion::Stale => (
                    TestState::Warning,
                    String::from("Job 'stale'?  No results."),
                ),
                Conclusion::StartupFailure => {
                    (TestState::Fail, String::from("Job currently broken."))
                }
                Conclusion::TimedOut => (TestState::Fail, String::from("Job timed out.")),
            },
            None => (TestState::Fail, String::from("Missing conclusion from job")),
        };

        Ok(RunnerResult {
            name: wfr.name.clone(),
            state,
            outcome,
            url: Some(wfr.html_url.clone()),
            description: Some(description),
        })
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
            let mut wfr = self.get_workflow_runs_for_branch(&branch_name)?;
            while Instant::now().duration_since(start) < timeout {
                if wfr.runs.is_empty() {
                    warn!("Branch {} has no workflows started!", branch_name);
                } else {
                    break;
                }
                thread::sleep(Duration::from_secs(30));
                wfr = self.get_workflow_runs_for_branch(&branch_name)?;
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
        let wfr = self.get_workflow_runs_for_branch(&branch_name)?;

        wfr.runs
            .par_iter()
            .map(|run| self.wfr_to_runner_result(run))
            .collect()
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

        let wfr: WorkflowRuns = gha.get_workflow_runs_for_branch("snowpatch/254076")?;

        println!("Found {} workflow runs.", wfr.count);

        wfr.runs.iter().for_each(|run| {
            println!(
                "Workflow {} has status {:?} and conclusion {:?} as {:?}",
                run.name,
                run.status,
                run.conclusion,
                gha.wfr_to_runner_result(&run)
            );
        });

        Ok(())
    }

    // TODO: migrate to runner.rs
    #[test]
    fn get_progress() -> Result<()> {
        let runner: Box<dyn Runner> = Box::new(get_gha());
        let jobs: Vec<RunnerResult> = runner.get_progress(&"snowpatch/254076".to_string(), None)?;
        println!("{:?}", jobs);

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
