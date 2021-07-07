// Attempt to define an API that runners have to implement
use crate::config::{Runner as RunnerConfig, Trigger};
use crate::DB;
use anyhow::Result;
use github::GitHubActions;
use ureq::Agent;
use url::Url;

pub mod github;

pub trait Runner {
    /// Set up any necessary state, and manually kick off tests if necessary.
    fn start_work(&self, url: Url, branch_name: String) -> Result<()>;
    /// For each spawned test, return status.  If tests are finished, should be enough to report.
    fn get_progress(&self, url: Url, branch_name: String) -> Result<String>;
    /// Assume this can be run at any time (i.e. other fatal failures) to clean up all state, local & remote.
    fn clean_up(&self, url: Url, branch_name: String) -> Result<()>;
}

// Exceedingly cursed type signature
pub fn init(config: Vec<RunnerConfig>, agent: Agent) -> Result<Vec<Box<dyn Runner>>> {
    let mut runners: Vec<Box<dyn Runner>> = vec![];
    // TODO very inelegant.
    let tree = DB.open_tree(b"remotes to push to")?;
    for runner in config {
        match runner {
            RunnerConfig::GitHub { trigger, url } => match trigger {
                Trigger::OnPush { remote } => {
                    tree.insert(remote.as_bytes(), b"github")?;
                    let gha = GitHubActions::new(
                        agent.clone(),
                        &url
                    )?;
                    runners.push(Box::new(gha));
                }

                Trigger::Manual { data: _ } => todo!(),
            },
        }
    }

    Ok(runners)
}
