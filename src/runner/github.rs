// Runner implementation for GitHub actions
// TODO initial support only targeting public GitHub
// TODO expecting jobs to spawn rather than manual triggers for now
use anyhow::{bail, Context, Error, Result};
use serde::Deserialize;
use ureq::{Agent, Response};
use url::Url;

use super::Runner;

pub struct GitHubActions {
    agent: Agent,
    api: Url
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

impl GitHubActions {
    pub fn new(agent: Agent, url: &Url) -> Result<GitHubActions> {
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
            api: api_url.clone()
        };

        // Smoke test to check the API URL works
        gha.api_req("GET", &api_url)
            .context(format!("Couldn't find branch with URL {}", &api_url))?;

        Ok(gha)
    }

    fn api_req(&self, method: &str, url: &Url) -> Result<Response> {
        let resp = self
            .agent
            .request_url(method, &url)
            .set("Accept", "application/vnd.github.v3+json")
            .call();

        let resp = resp?;

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
        runs_url
            .query_pairs_mut()
            .append_pair("branch", branch);

        Ok(serde_json::from_value(
            self.api_req("GET", &runs_url)?.into_json()?,
        )?)
    }
}

impl Runner for GitHubActions {
    fn start_work(&self, _url: Url, _branch_name: String) -> Result<()> {
        todo!()
    }

    fn get_progress(&self, _url: Url, _branch_name: String) -> Result<String> {
        todo!()
    }

    fn clean_up(&self, _url: Url, _branch_name: String) -> Result<()> {
        todo!()
    }
}

mod tests {
    use super::*;
    use anyhow::Result;
    use ureq::Agent;

    fn get_gha() -> GitHubActions {
        GitHubActions::new(Agent::new(), &Url::parse("https://github.com/ruscur/linux-ci").unwrap()).unwrap()
    }

    #[test]
    fn get_actions() -> Result<()> {
        let gha = get_gha();

        let wfr: WorkflowRuns = gha.get_workflow_runs("nice")?;

        println!("Found {} workflow runs.", wfr.count);

        wfr.runs.iter().for_each(|run| {
            println!(
                "Workflow {} has status {:?} and conclusion {:?}",
                run.name, run.status, run.conclusion
            );
        });

        Ok(())
    }
}
