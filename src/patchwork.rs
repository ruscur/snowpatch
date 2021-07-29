/// the patchwork module should not track state about any patch
/// it should handle all direct API interactions and common operations on the objects it returns
/// basically, if there's any part of snowpatch that could become its own individual library, it's this.
use anyhow::bail;
use anyhow::{Context, Error, Result};
use log::log_enabled;
use log::{debug, warn};
use rayon::iter::IntoParallelRefIterator;
use rayon::prelude::*;
use serde::{self, Deserialize, Serialize, Serializer};
use std::collections::BTreeMap;
use std::io::Read;
use ureq::json;
use ureq::Agent;
use url::Url;

#[derive(Clone)]
pub struct PatchworkServer {
    api: Url,
    token: Option<String>,
    agent: Agent,
}

impl PatchworkServer {
    pub fn new(url: Url, token: Option<String>, agent: Agent) -> Result<PatchworkServer> {
        let mut api_url = url.clone();

        api_url
            .path_segments_mut() // Each segment of the URL path
            .map_err(|_| Error::msg("URL is boned"))? // URL crate sucks
            .push("api")
            .push("1.2"); // snowpatch will only ever support one revision

        let server = PatchworkServer {
            api: api_url,
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
            .append_pair("per_page", "8") // this is max for ozlabs.org XXX TODO 250 is max
            .append_pair("page", "3")
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

    pub fn send_check(&self, series: u64, result: &TestResult) -> Result<()> {
        if self.token.is_none() {
            warn!(
                "Couldn't send result for {} since we don't have a token.",
                series
            );
            return Ok(());
        }

        let series = self.get_series(series)?;
        let patch = series
            .patches
            .last()
            .context("We got this far with a series with no patches?")?;
        let encoded = serde_json::to_value(&result)?;

        let mut check_url = self.api.clone();

        check_url
            .path_segments_mut()
            .map_err(|_| Error::msg("URL is boned"))? // URL crate sucks
            .push("patches")
            .push(&patch.id.to_string())
            .push("checks");

        // Why yes, I did just use a URL construction API, which is complete overkill,
        // just to have to manually append a trailing slash.
        // Patchwork is love.  Patchwork is life.
        let check_url = format!("{}/", check_url.to_string());

        let resp = self
            .agent
            .request("POST", &check_url)
            .set("Accept", "application/json")
            .set(
                "Authorization",
                &format!("Token {}", self.token.as_ref().unwrap()),
            )
            .send_json(encoded)?
            .into_string()?;

        dbg!(resp);

        Ok(())
    }
}

/// Just download a thing.  Designed for downloading patches.
/// Doesn't need any state since there's no auth involved.
/// *Could* need some state for the agent if there were proxies
/// involved, also could need some state for performance if the
/// agent connection pool actually matters.
pub fn download_file(url: &Url) -> Result<Vec<u8>> {
    let req = ureq::request_url("GET", &url).call()?;

    let mut buf: Vec<u8> = vec![];

    req.into_reader().read_to_end(&mut buf)?;

    Ok(buf)
}

#[derive(Deserialize, Clone, Debug)]
pub struct SubmitterSummary {
    pub id: u64,
    pub url: Url,
    pub name: Option<String>,
    pub email: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct DelegateSummary {
    pub id: u64,
    pub url: Url,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserSummary {
    pub id: u64,
    pub url: Url,
    pub username: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
}

// /api/1.2/projects/{id}
#[derive(Deserialize, Clone, Debug)]
pub struct Project {
    pub id: u64,
    pub url: Url,
    pub name: String,
    pub link_name: String,
    pub list_email: String,
    pub list_id: String,
    pub web_url: Option<Url>,
    pub scm_url: Option<Url>,
    pub webscm_url: Option<Url>,
}

// /api/1.2/patches/
// This omits fields from /patches/{id}, deal with it for now.

#[derive(Deserialize, Clone, Debug)]
pub struct Patch {
    pub id: u64,
    pub url: Url,
    pub project: Project,
    pub msgid: String,
    pub date: String,
    pub name: String,
    pub commit_ref: Option<String>,
    pub pull_url: Option<Url>,
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
        self.pull_url.is_none() && (&self.state == "new" || &self.state == "under-review")
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct PatchSummary {
    pub date: String,
    pub id: u64,
    pub mbox: Url,
    pub msgid: String,
    pub name: String,
    pub url: Url,
}

#[derive(Deserialize, Clone, Debug)]
pub struct CoverLetter {
    pub date: String,
    pub id: u64,
    pub msgid: String,
    pub name: String,
    pub url: Url,
}

// /api/1.2/series/
// The series list and /series/{id} are the same, luckily
#[derive(Deserialize, Clone, Debug)]
pub struct Series {
    pub cover_letter: Option<CoverLetter>,
    pub date: String,
    pub id: u64,
    pub mbox: Url,
    pub name: Option<String>,
    pub patches: Vec<PatchSummary>,
    pub project: Project,
    pub received_all: bool,
    pub received_total: u64,
    pub submitter: SubmitterSummary,
    pub total: u64,
    pub url: Url,
    pub version: u64,
}

#[derive(Deserialize, Clone, Debug)]
pub struct SeriesSummary {
    pub id: u64,
    pub url: Url,
    pub date: String,
    pub name: Option<String>,
    pub version: u64,
    pub mbox: Url,
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
            .timeout_read(Duration::from_secs(30))
            .timeout_write(Duration::from_secs(90))
            .build()
    }

    fn init() {
        let _ = env_logger::builder().is_test(true).try_init();
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

    fn create_server_object() -> Result<PatchworkServer, anyhow::Error> {
        let pws = PatchworkServer::new(Url::parse(&PATCHWORK_BASE_URL)?, None, test_get_agent())?;

        Ok(pws)
    }

    #[test]
    fn parse_patch() -> Result<(), anyhow::Error> {
        let server = create_server_object()?;

        let patch = server.get_patch(GOOD_PATCH_ID)?;

        dbg!(patch);

        Ok(())
    }

    #[test]
    fn parse_series() -> Result<(), anyhow::Error> {
        let server = create_server_object()?;

        let series = server.get_series(GOOD_SERIES_ID)?;

        dbg!(series);

        Ok(())
    }

    #[test]
    fn get_bad_patch() -> () {
        match create_server_object().unwrap().get_patch(u64::MAX) {
            Ok(_) => {
                panic!("get_patch() succeded on bad patch!")
            }
            Err(_) => return (),
        }
    }

    #[test]
    fn parse_series_list() -> Result<(), anyhow::Error> {
        let server = create_server_object()?;

        let list = server.get_series_list(GOOD_PATCHWORK_PROJECT)?;

        assert_eq!(list.len(), 250);

        Ok(())
    }

    #[test]
    #[ignore]
    fn send_check() -> Result<(), anyhow::Error> {
        let token = "PUT TOKEN HERE".to_string();
        let server = PatchworkServer::new(
            Url::parse(&PATCHWORK_BASE_URL)?,
            Some(token),
            test_get_agent(),
        )?;
        let result = TestResult {
            state: TestState::Success,
            target_url: None,
            description: None,
            context: None,
        };

        server.send_check(GOOD_SERIES_ID, &result)
    }
}
