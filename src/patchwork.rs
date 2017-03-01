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
use std::io::{self};
use std::option::Option;
use std::path::PathBuf;
use std::fs::File;
use std::result::Result;

use tempdir::TempDir;

use hyper;
use hyper::Client;
use hyper::header::{Connection, Headers, Accept, ContentType, qitem,
                    Authorization, Basic};
use hyper::mime::{Mime, TopLevel, SubLevel, Attr, Value};
use hyper::status::StatusCode;
use hyper::client::response::Response;

use serde_json;

use utils;

// TODO: more constants.  constants for format strings of URLs and such.
pub static PATCHWORK_API: &'static str = "/api/1.0";
pub static PATCHWORK_QUERY: &'static str = "?ordering=-last_updated&related=expand";

// /api/1.0/projects/*/series/

#[derive(Deserialize, Clone)]
pub struct Project {
    pub id: u64,
    pub name: String,
    pub linkname: String,
    pub listemail: String,
    pub web_url: Option<String>,
    pub scm_url: Option<String>,
    pub webscm_url: Option<String>
}

#[derive(Deserialize, Clone)]
pub struct Submitter {
    pub id: u64,
    pub name: String
}

#[derive(Deserialize, Clone)]
pub struct Series {
    pub id: u64,
    pub project: Project,
    pub name: String,
    pub n_patches: u64,
    pub submitter: Submitter,
    pub submitted: String,
    pub last_updated: String,
    pub version: u64,
    pub reviewer: Option<String>,
    pub test_state: Option<String>
}

#[derive(Deserialize)]
pub struct SeriesList {
    pub count: u64,
    pub next: Option<String>,
    pub previous: Option<String>,
    pub results: Option<Vec<Series>>
}

#[derive(Serialize, Clone)]
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
#[derive(Serialize)]
pub struct TestResult {
    pub test_name: String,
    pub state: TestState,
    pub url: Option<String>,
    pub summary: Option<String>
}

pub struct PatchworkServer {
    pub url: String,
    headers: hyper::header::Headers,
    pub client: std::sync::Arc<Client>,
}

impl PatchworkServer {
    #[cfg_attr(feature="cargo-clippy", allow(ptr_arg))]
    pub fn new(url: &String, client: &std::sync::Arc<Client>) -> PatchworkServer {
        let mut headers = Headers::new();
        headers.set(Accept(vec![qitem(Mime(TopLevel::Application,
                                           SubLevel::Json,
                                           vec![(Attr::Charset, Value::Utf8)]))])
        );
        headers.set(ContentType(Mime(TopLevel::Application,
                                     SubLevel::Json,
                                     vec![(Attr::Charset, Value::Utf8)]))
        );
        PatchworkServer {
            url: url.clone(),
            client: client.clone(),
            headers: headers,
        }
    }

    #[cfg_attr(feature="cargo-clippy", allow(ptr_arg))]
    pub fn set_authentication(&mut self, username: &String, password: &Option<String>) {
        self.headers.set(Authorization(Basic {
            username: username.clone(),
            password: password.clone(),
        }));
    }

    fn get(&self, url: &str) -> std::result::Result<String, hyper::error::Error> {
        let mut resp = try!(self.client.get(&*url).headers(self.headers.clone())
                            .header(Connection::close()).send());
        let mut body: Vec<u8> = vec![];
        io::copy(&mut resp, &mut body).unwrap();
        Ok(String::from_utf8(body).unwrap())
    }

    pub fn post_test_result(&self, result: TestResult,
                            series_id: &u64, series_revision: &u64)
                            -> Result<StatusCode, hyper::error::Error> {
        let encoded = serde_json::to_string(&result).unwrap();
        let headers = self.headers.clone();
        debug!("JSON Encoded: {}", encoded);
        let res = try!(self.client.post(&format!(
            "{}{}/series/{}/revisions/{}/test-results/",
            &self.url, PATCHWORK_API, &series_id, &series_revision))
            .headers(headers).body(&encoded).send());
        assert_eq!(res.status, hyper::status::StatusCode::Created);
        Ok(res.status)
    }

    pub fn get_series(&self, series_id: &u64) -> Result<Series, serde_json::Error> {
        let url = format!("{}{}/series/{}{}", &self.url, PATCHWORK_API,
                          series_id, PATCHWORK_QUERY);
        serde_json::from_str(&self.get(&url).unwrap())
    }

    pub fn get_series_mbox(&self, series_id: &u64, series_revision: &u64)
                           -> std::result::Result<Response, hyper::error::Error> {
        let url = format!("{}{}/series/{}/revisions/{}/mbox/",
                               &self.url, PATCHWORK_API, series_id, series_revision);
        self.client.get(&*url).headers(self.headers.clone())
            .header(Connection::close()).send()
    }

    pub fn get_series_query(&self) -> Result<SeriesList, serde_json::Error> {
        let url = format!("{}{}/series/{}", &self.url,
                          PATCHWORK_API, PATCHWORK_QUERY);
        serde_json::from_str(&self.get(&url).unwrap_or_else(
            |err| panic!("Failed to connect to Patchwork: {}", err)))
    }

    pub fn get_patch(&self, series: &Series) -> PathBuf {
        let dir = TempDir::new("snowpatch").unwrap().into_path();
        let mut path = dir.clone();
        let tag = utils::sanitise_path(
            format!("{}-{}-{}", series.submitter.name,
                    series.id, series.version));
        path.push(format!("{}.mbox", tag));

        let mut mbox_resp = self.get_series_mbox(&series.id, &series.version)
            .unwrap();

        debug!("Saving patch to file {}", path.display());
        let mut mbox = File::create(&path).unwrap_or_else(
            |err| panic!("Couldn't create mbox file: {}", err));
        io::copy(&mut mbox_resp, &mut mbox).unwrap_or_else(
            |err| panic!("Couldn't save mbox from Patchwork: {}", err));
        path
    }
}
