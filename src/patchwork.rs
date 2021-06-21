use anyhow::{Error, Result};
use log::debug;
use rayon::iter::IntoParallelRefIterator;
use rayon::prelude::*;
use serde::{self, Deserialize, Serialize, Serializer};
use std::collections::BTreeMap;
use ureq::Agent;
use url::Url;

pub struct PatchworkServer {
    api: Url,
    token: Option<String>,
    agent: Agent,
}

impl PatchworkServer {
    pub fn new(url: String, token: Option<String>, agent: Agent) -> Result<PatchworkServer> {
        let mut url_struct = Url::parse(&url)?;

        url_struct
            .path_segments_mut() // Each segment of the URL path
            .map_err(|_| Error::msg("URL is boned"))? // URL crate sucks
            .push("api")
            .push("1.2"); // snowpatch will only ever support one revision

        let server = PatchworkServer {
            api: url_struct,
            token,
            agent,
        };

        server.smoke_test()?;

        Ok(server)
    }

    fn smoke_test(&self) -> Result<()> {
        let req = self.agent.request_url("GET", &self.api).call()?;

        debug!("{:?}", req.into_string()?);

        Ok(())
    }

    pub fn get_patch(&self, id: u64) -> Result<Patch> {
        let mut patch_url = self.api.clone();
        patch_url
            .path_segments_mut()
            .map_err(|_| Error::msg("URL is boned"))? // URL crate sucks
            .push("patches")
            .push(&id.to_string());

        let resp = self.agent.request_url("GET", &patch_url).call()?;

        Ok(serde_json::from_value(resp.into_json()?)?)
    }

    pub fn get_series(&self, id: u64) -> Result<Series> {
        let mut series_url = self.api.clone();
        series_url
            .path_segments_mut()
            .map_err(|_| Error::msg("URL is boned"))? // URL crate sucks
            .push("series")
            .push(&id.to_string());

        let resp = self.agent.request_url("GET", &series_url).call()?;

        Ok(serde_json::from_value(resp.into_json()?)?)
    }

    /// Be careful how often this is run, makes Patchwork do lots of work.
    pub fn get_series_list(&self, project: &str) -> Result<Vec<Series>> {
        let mut series_list_url = self.api.clone();

        series_list_url
            .path_segments_mut()
            .map_err(|_| Error::msg("URL is boned"))? // URL crate sucks
            .push("series");

        series_list_url
            .query_pairs_mut()
            .append_pair("order", "-id") // newest series at the top
            .append_pair("per_page", "250") // this is max for ozlabs.org
            .append_pair("project", project);

        let resp = self.agent.request_url("GET", &series_list_url).call()?;

        Ok(serde_json::from_value(resp.into_json()?)?)
    }

    //pub fn get_patch_state(&self, patch: u64) -> Result<Vec<TestResult>>;

    pub fn get_patch_checks(&self, patch: u64) -> Result<Vec<Check>> {
        let mut patch_checks_url = self.api.clone();

        patch_checks_url
            .path_segments_mut()
            .map_err(|_| Error::msg("URL is boned"))? // URL crate sucks
            .push("patches")
            .push(&patch.to_string())
            .push("checks");

        let resp = self.agent.request_url("GET", &&patch_checks_url).call()?;

        Ok(serde_json::from_value(resp.into_json()?)?)
    }

    pub fn get_series_state(&self, series: u64) -> Result<TestState> {
        let series = self.get_series(series)?;

        let patches: Result<Vec<Patch>> = series
            .patches
            .par_iter()
            .map(|p| -> Result<Patch> { self.get_patch(p.id) })
            .collect();

        let patches: Vec<Patch> = patches?;

        let check_status: Vec<TestState> = patches.iter().map(|p| p.check.clone()).collect();

        if check_status.contains(&TestState::Pending) {
            Ok(TestState::Pending)
        } else if check_status.contains(&TestState::Fail) {
            Ok(TestState::Fail)
        } else if check_status.contains(&TestState::Warning) {
            Ok(TestState::Warning)
        } else {
            Ok(TestState::Success)
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct SubmitterSummary {
    pub id: u64,
    pub url: String,
    pub name: Option<String>,
    pub email: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct DelegateSummary {
    pub id: u64,
    pub url: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserSummary {
    pub id: u64,
    pub url: String,
    pub username: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
}

// /api/1.2/projects/{id}
#[derive(Deserialize, Clone, Debug)]
pub struct Project {
    pub id: u64,
    pub url: String,
    pub name: String,
    pub link_name: String,
    pub list_email: String,
    pub list_id: String,
    pub web_url: Option<String>,
    pub scm_url: Option<String>,
    pub webscm_url: Option<String>,
}

// /api/1.2/patches/
// This omits fields from /patches/{id}, deal with it for now.

#[derive(Deserialize, Clone, Debug)]
pub struct Patch {
    pub id: u64,
    pub url: String,
    pub project: Project,
    pub msgid: String,
    pub date: String,
    pub name: String,
    pub commit_ref: Option<String>,
    pub pull_url: Option<String>,
    pub state: String, // TODO enum of possible states
    pub archived: bool,
    pub hash: Option<String>,
    pub submitter: SubmitterSummary,
    pub delegate: Option<DelegateSummary>,
    pub mbox: String,
    pub series: Vec<SeriesSummary>,
    pub check: TestState,
    pub checks: String, // URL
    pub tags: BTreeMap<String, u64>,
}

impl Patch {
    pub fn has_series(&self) -> bool {
        !&self.series.is_empty()
    }

    pub fn action_required(&self) -> bool {
        &self.state == "new" || &self.state == "under-review"
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct PatchSummary {
    pub date: String,
    pub id: u64,
    pub mbox: String,
    pub msgid: String,
    pub name: String,
    pub url: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct CoverLetter {
    pub date: String,
    pub id: u64,
    pub msgid: String,
    pub name: String,
    pub url: String,
}

// /api/1.2/series/
// The series list and /series/{id} are the same, luckily
#[derive(Deserialize, Clone, Debug)]
pub struct Series {
    pub cover_letter: Option<CoverLetter>,
    pub date: String,
    pub id: u64,
    pub mbox: String,
    pub name: Option<String>,
    pub patches: Vec<PatchSummary>,
    pub project: Project,
    pub received_all: bool,
    pub received_total: u64,
    pub submitter: SubmitterSummary,
    pub total: u64,
    pub url: String,
    pub version: u64,
}

#[derive(Deserialize, Clone, Debug)]
pub struct SeriesSummary {
    pub id: u64,
    pub url: String,
    pub date: String,
    pub name: Option<String>,
    pub version: u64,
    pub mbox: String,
}

#[derive(Deserialize, Serialize, Clone, PartialEq, Debug)]
pub enum TestState {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "fail")]
    Fail,
}

impl Default for TestState {
    fn default() -> TestState {
        TestState::Pending
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Check {
    pub id: u64,
    pub url: String,
    pub user: UserSummary,
    pub date: String,
    pub state: TestState,
    pub target_url: Option<String>,
    pub context: String,
    pub description: Option<String>,
}

// POST to /api/1.2/patches/{patch_id}/checks/
#[derive(Serialize, Default, Clone, Debug)]
pub struct TestResult {
    pub state: TestState,
    pub target_url: Option<String>,
    pub description: Option<String>,
    #[serde(serialize_with = "TestResult::serialize_context")]
    pub context: Option<String>,
}

impl TestResult {
    fn serialize_context<S>(context: &Option<String>, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if context.is_none() {
            serde::Serialize::serialize(
                &Some(
                    format!("{}-{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
                        .to_string()
                        .replace(".", "_"),
                ),
                ser,
            )
        } else {
            serde::Serialize::serialize(context, ser)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use ureq::{Agent, AgentBuilder, OrAnyStatus};

    // These are all based around one Patchwork instance.
    // If the server is down, or stuff gets deleted, they will fail.
    // We're not bundling a mock Patchwork API into snowpatch so it'll do.
    static PATCHWORK_API_URL: &'static str = "https://patchwork.ozlabs.org/api/1.2";
    static PATCHWORK_BASE_URL: &'static str = "https://patchwork.ozlabs.org";
    static GOOD_PATCHWORK_PROJECT: &'static str = "linuxppc-dev";
    static GOOD_PATCH_ID: u64 = 552023;
    static GOOD_SERIES_ID: u64 = 13675;

    fn test_get_agent() -> Agent {
        AgentBuilder::new()
            .timeout_read(Duration::from_secs(10))
            .timeout_write(Duration::from_secs(10))
            .build()
    }

    #[test]
    fn get_api_version() -> Result<(), ureq::Error> {
        let agent = test_get_agent();

        let resp = agent.get(PATCHWORK_API_URL).call()?;

        assert_eq!(
            resp.status(),
            (200 as u16),
            "Patchwork API didn't return 200"
        );

        Ok(())
    }

    #[test]
    fn get_bad_api_version() -> Result<(), ureq::Error> {
        let agent = test_get_agent();

        let mut url = String::from(PATCHWORK_BASE_URL);
        url.push_str("/api/6.9");

        let resp = agent.get(&url).call().or_any_status()?;

        assert_eq!(
            resp.status(),
            (404 as u16),
            "Patchwork didn't return 404 on bad API version"
        );

        Ok(())
    }

    #[test]
    fn create_server_object() -> Result<(), anyhow::Error> {
        PatchworkServer::new(PATCHWORK_BASE_URL.to_string(), None, test_get_agent())?;

        Ok(())
    }

    #[test]
    fn parse_patch() -> Result<(), anyhow::Error> {
        let server = PatchworkServer::new(PATCHWORK_BASE_URL.to_string(), None, test_get_agent())?;

        let patch = server.get_patch(GOOD_PATCH_ID)?;

        dbg!(patch);

        Ok(())
    }

    #[test]
    fn parse_series() -> Result<(), anyhow::Error> {
        let server = PatchworkServer::new(PATCHWORK_BASE_URL.to_string(), None, test_get_agent())?;

        let series = server.get_series(GOOD_SERIES_ID)?;

        dbg!(series);

        Ok(())
    }

    #[test]
    #[should_panic]
    fn get_bad_patch() -> () {
        let server =
            PatchworkServer::new(PATCHWORK_BASE_URL.to_string(), None, test_get_agent()).unwrap();

        let patch = server.get_patch(u64::MAX).unwrap();
    }

    #[test]
    fn parse_series_list() -> Result<(), anyhow::Error> {
        let server =
            PatchworkServer::new(PATCHWORK_BASE_URL.to_string(), None, test_get_agent()).unwrap();

        let list = server.get_series_list(GOOD_PATCHWORK_PROJECT)?;

        assert_eq!(list.len(), 250);

        Ok(())
    }
}
