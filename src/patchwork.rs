//
// snowpatch - continuous integration for patch-based workflows
//
// Copyright (C) 2016 IBM Corporation
// Authors:
//     Russell Currey <ruscur@russell.cc>
//     Andrew Donnellan <andrew.donnellan@au1.ibm.com>
//
// This program is free software; you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation; either version 2 of the License, or (at your option)
// any later version.
//
// patchwork.rs - patchwork API
//

use std;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io;
use std::option::Option;
use std::path::PathBuf;
use std::result::Result;

use tempdir::TempDir;

use reqwest;
use reqwest::header::{
    qitem, Accept, Authorization, Basic, Connection, ContentType, Headers, Link, RelationType,
};
use reqwest::Client;
use reqwest::Response;
use reqwest::StatusCode;

use serde::{self, Serializer};
use serde_json;

use utils;

// TODO: more constants.  constants for format strings of URLs and such.
pub static PATCHWORK_API: &'static str = "/api/1.0";
pub static PATCHWORK_QUERY: &'static str = "?order=-id";

#[derive(Deserialize, Clone)]
pub struct SubmitterSummary {
    pub id: u64,
    pub url: String,
    pub name: String,
    pub email: String,
}

#[derive(Deserialize, Clone)]
pub struct DelegateSummary {
    pub id: u64,
    pub url: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
}

// /api/1.0/projects/{id}
#[derive(Deserialize, Clone)]
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

// /api/1.0/patches/
// This omits fields from /patches/{id}, deal with it for now.

#[derive(Deserialize, Clone)]
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
    pub check: String, // TODO enum of possible states
    pub checks: String,
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

#[derive(Deserialize, Clone)]
pub struct PatchSummary {
    pub date: String,
    pub id: u64,
    pub mbox: String,
    pub msgid: String,
    pub name: String,
    pub url: String,
}

#[derive(Deserialize, Clone)]
pub struct CoverLetter {
    pub date: String,
    pub id: u64,
    pub msgid: String,
    pub name: String,
    pub url: String,
}

// /api/1.0/series/
// The series list and /series/{id} are the same, luckily
#[derive(Deserialize, Clone)]
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

#[derive(Deserialize, Clone)]
pub struct SeriesSummary {
    pub id: u64,
    pub url: String,
    pub date: String,
    pub name: Option<String>,
    pub version: u64,
    pub mbox: String,
}

#[derive(Serialize, Clone, PartialEq)]
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

// /api/1.0/series/*/revisions/*/test-results/
#[derive(Serialize, Default, Clone)]
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

pub struct PatchworkServer {
    pub url: String,
    headers: Headers,
    pub client: std::sync::Arc<Client>,
}

impl PatchworkServer {
    #[cfg_attr(feature = "cargo-clippy", allow(ptr_arg))]
    pub fn new(url: &String, client: &std::sync::Arc<Client>) -> PatchworkServer {
        let mut headers = Headers::new();
        headers.set(Accept(vec![qitem(reqwest::mime::APPLICATION_JSON)]));
        headers.set(ContentType(reqwest::mime::APPLICATION_JSON));
        PatchworkServer {
            url: url.clone(),
            client: client.clone(),
            headers: headers,
        }
    }

    #[cfg_attr(feature = "cargo-clippy", allow(ptr_arg))]
    pub fn set_authentication(
        &mut self,
        username: &Option<String>,
        password: &Option<String>,
        token: &Option<String>,
    ) {
        match (username, password, token) {
            (&None, &None, &Some(ref token)) => {
                self.headers.set(Authorization(format!("Token {}", token)));
            }
            (&Some(ref username), &Some(ref password), &None) => {
                self.headers.set(Authorization(Basic {
                    username: username.clone(),
                    password: Some(password.clone()),
                }));
            }
            _ => panic!("Invalid patchwork authentication details"),
        }
    }

    pub fn get_url(&self, url: &str) -> std::result::Result<Response, reqwest::Error> {
        self.client
            .get(&*url)
            .headers(self.headers.clone())
            .header(Connection::close())
            .send()
    }

    pub fn get_url_string(&self, url: &str) -> std::result::Result<String, reqwest::Error> {
        let mut resp = try!(
            self.client
                .get(&*url)
                .headers(self.headers.clone())
                .header(Connection::close())
                .send()
        );
        let mut body: Vec<u8> = vec![];
        io::copy(&mut resp, &mut body).unwrap();
        Ok(String::from_utf8(body).unwrap())
    }

    pub fn post_test_result(
        &self,
        result: TestResult,
        checks_url: &str,
    ) -> Result<StatusCode, reqwest::Error> {
        let encoded = serde_json::to_string(&result).unwrap();
        let headers = self.headers.clone();
        debug!("JSON Encoded: {}", encoded);
        let mut resp = try!(
            self.client
                .post(checks_url)
                .headers(headers)
                .body(encoded)
                .send()
        );
        let mut body: Vec<u8> = vec![];
        io::copy(&mut resp, &mut body).unwrap();
        trace!("{}", String::from_utf8(body).unwrap());
        assert_eq!(resp.status(), StatusCode::Created);
        Ok(resp.status())
    }

    pub fn get_patch(&self, patch_id: &u64) -> Result<Patch, serde_json::Error> {
        let url = format!(
            "{}{}/patches/{}{}",
            &self.url, PATCHWORK_API, patch_id, PATCHWORK_QUERY
        );
        serde_json::from_str(&self.get_url_string(&url).unwrap())
    }

    pub fn get_patch_by_url(&self, url: &str) -> Result<Patch, serde_json::Error> {
        serde_json::from_str(&self.get_url_string(url).unwrap())
    }

    pub fn get_patch_query(&self, project: &str) -> Result<Vec<Patch>, serde_json::Error> {
        let url = format!(
            "{}{}/patches/{}&project={}",
            &self.url, PATCHWORK_API, PATCHWORK_QUERY, project
        );

        serde_json::from_str(&self.get_url_string(&url)
            .unwrap_or_else(|err| panic!("Failed to connect to Patchwork: {}", err)))
    }

    fn get_next_link(&self, resp: &Response) -> Option<String> {
        let next = resp.headers().get::<Link>()?;
        for val in next.values() {
            if let Some(rel) = val.rel() {
                if rel.iter().any(|reltype| reltype == &RelationType::Next) {
                    return Some(val.link().to_string());
                }
            }
        }
        None
    }

    pub fn get_patch_query_num(
        &self,
        project: &str,
        num_patches: usize,
    ) -> Result<Vec<Patch>, serde_json::Error> {
        let mut list: Vec<Patch> = vec![];
        let mut url = Some(format!(
            "{}{}/patches/{}&project={}",
            &self.url, PATCHWORK_API, PATCHWORK_QUERY, project
        ));

        while let Some(real_url) = url {
            let resp = self.get_url(&real_url)
                .unwrap_or_else(|err| panic!("Failed to connect to Patchwork: {}", err));
            url = self.get_next_link(&resp);
            let new_patches: Vec<Patch> = serde_json::from_reader(resp)?;
            list.extend(new_patches);
            if list.len() >= num_patches {
                break;
            }
        }
        list.truncate(num_patches);
        Ok(list)
    }

    pub fn get_patch_dependencies(&self, patch: &Patch) -> Vec<Patch> {
        // We assume the list of patches in a series are in order.
        let mut dependencies: Vec<Patch> = vec![];
        let series = self.get_series_by_url(&patch.series[0].url);
        if series.is_err() {
            return dependencies;
        }
        for dependency in series.unwrap().patches {
            dependencies.push(self.get_patch_by_url(&dependency.url).unwrap());
            if dependency.url == patch.url {
                break;
            }
        }
        dependencies
    }

    pub fn get_patch_mbox(&self, patch: &Patch) -> PathBuf {
        let dir = TempDir::new("snowpatch").unwrap().into_path();
        let mut path = dir.clone();
        let tag = utils::sanitise_path(patch.name.clone());
        path.push(format!("{}.mbox", tag));

        let mut mbox_resp = self.get_url(&patch.mbox).unwrap();

        debug!("Saving patch to file {}", path.display());
        let mut mbox =
            File::create(&path).unwrap_or_else(|err| panic!("Couldn't create mbox file: {}", err));
        io::copy(&mut mbox_resp, &mut mbox)
            .unwrap_or_else(|err| panic!("Couldn't save mbox from Patchwork: {}", err));
        path
    }

    pub fn get_patches_mbox(&self, patches: Vec<Patch>) -> PathBuf {
        let dir = TempDir::new("snowpatch").unwrap().into_path();
        let mut path = dir.clone();
        let tag = utils::sanitise_path(patches.last().unwrap().name.clone());
        path.push(format!("{}.mbox", tag));

        let mut mbox = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&path)
            .unwrap_or_else(|err| panic!("Couldn't make file: {}", err));

        for patch in patches {
            let mut mbox_resp = self.get_url(&patch.mbox).unwrap();
            debug!("Appending patch {} to file {}", patch.name, path.display());
            io::copy(&mut mbox_resp, &mut mbox)
                .unwrap_or_else(|err| panic!("Couldn't save mbox from Patchwork: {}", err));
        }
        path
    }

    pub fn get_series(&self, series_id: &u64) -> Result<Series, serde_json::Error> {
        let url = format!(
            "{}{}/series/{}{}",
            &self.url, PATCHWORK_API, series_id, PATCHWORK_QUERY
        );
        serde_json::from_str(&self.get_url_string(&url).unwrap())
    }

    pub fn get_series_by_url(&self, url: &str) -> Result<Series, serde_json::Error> {
        serde_json::from_str(&self.get_url_string(url).unwrap())
    }
}
