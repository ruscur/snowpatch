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
use std::fs::{File, OpenOptions};
use std::result::Result;
use std::collections::BTreeMap;

use tempdir::TempDir;

// TODO: this line is required here, but why?
extern crate hyper;
use hyper::Client;
use hyper::header::{Connection, Headers, Accept, ContentType, qitem, Authorization, Basic};
use hyper::mime::{Mime, TopLevel, SubLevel, Attr, Value};
use hyper::status::StatusCode;
use hyper::client::response::Response;

use rustc_serialize::json::{self, Json, ToJson, DecoderError};

use utils;

// TODO: more constants.  constants for format strings of URLs and such.
pub static PATCHWORK_API: &'static str = "/api/1.0";
pub static PATCHWORK_QUERY: &'static str = "?page=last";

// /api/1.0/projects/{id}
#[derive(RustcDecodable, Clone)]
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
    pub maintainers: Vec<String>
}

// /api/1.0/patches/
// This omits fields from /patches/{id}, deal with it for now.

#[derive(RustcDecodable, Clone)]
pub struct Patch {
    pub id: u64,
    pub url: String,
    pub project: String,
    pub msgid: String,
    pub date: String,
    pub name: String,
    pub commit_ref: Option<String>,
    pub pull_url: Option<String>,
    pub state: String, // TODO enum of possible states
    pub archived: bool,
    pub hash: Option<String>,
    pub submitter: String,
    pub delegate: Option<String>,
    pub mbox: String,
    pub series: Vec<String>,
    pub check: String, // TODO enum of possible states
    pub checks: String,
    pub tags: BTreeMap<String, u64>
}

impl Patch {
    pub fn has_series(&self) -> bool {
            !&self.series.is_empty()
    }
}

// /api/1.0/series/
// The series list and /series/{id} are the same, luckily
#[derive(RustcDecodable, Clone)]
pub struct Series {
    pub id: u64,
    pub url: String,
    pub name: String,
    pub date: String,
    pub submitter: String,
    pub version: u64,
    pub total: u64,
    pub received_total: u64,
    pub received_all: bool,
    pub cover_letter: Option<String>,
    pub patches: Vec<String>
}

// TODO: remove this when we have Jenkins result handling
#[allow(dead_code)]
#[derive(RustcEncodable, Clone)]
pub enum TestState {
    PENDING,
    SUCCESS,
    WARNING,
    FAIL,
}

impl ToJson for TestState {
    fn to_json(&self) -> Json {
        Json::String(
            match *self {
                TestState::PENDING => "pending".to_string(),
                TestState::SUCCESS => "success".to_string(),
                TestState::WARNING => "warning".to_string(),
                TestState::FAIL    => "fail".to_string(),
            }
        )
    }
}

impl Default for TestState {
    fn default() -> TestState {
        TestState::PENDING
    }
}

// /api/1.0/series/*/revisions/*/test-results/
#[derive(RustcEncodable, Default, Clone)]
pub struct TestResult {
    pub state: TestState,
    pub target_url: Option<String>,
    pub description: Option<String>,
    pub context: Option<String>,
}

impl TestResult {
    pub fn as_json(&self) -> String {
        let mut result = self.clone();
        if result.target_url.is_none() {
            result.target_url = Some("http://no.url".to_string());
        }
        if result.context.is_none() {
            result.context = Some(format!("{}-{}",
                                     env!("CARGO_PKG_NAME"),
                                     env!("CARGO_PKG_VERSION")).to_string()
                .replace(".", "_"));
        }
        json::encode(&result).unwrap()
    }
}

pub struct PatchworkServer {
    pub url: String,
    headers: hyper::header::Headers,
    pub client: std::sync::Arc<Client>,
}

impl PatchworkServer {
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

    pub fn set_authentication(&mut self, username: &String, password: &Option<String>) {
        self.headers.set(Authorization(Basic {
            username: username.clone(),
            password: password.clone(),
        }));
    }
    pub fn get_url(&self, url: &String)
                   -> std::result::Result<Response, hyper::error::Error> {
        self.client.get(&*url).headers(self.headers.clone())
            .header(Connection::close()).send()
    }

    pub fn get_url_string(&self, url: &String) -> std::result::Result<String, hyper::error::Error> {
        let mut resp = try!(self.client.get(&*url).headers(self.headers.clone())
                            .header(Connection::close()).send());
        let mut body: Vec<u8> = vec![];
        io::copy(&mut resp, &mut body).unwrap();
        Ok(String::from_utf8(body).unwrap())
    }


    pub fn post_test_result(&self, result: TestResult, checks_url: &String)
                            -> Result<StatusCode, hyper::error::Error> {
        let encoded = result.as_json();
        let headers = self.headers.clone();
        debug!("JSON Encoded: {}", encoded);
        let mut resp = try!(self.client.post(checks_url)
                        .headers(headers).body(&encoded).send());
        let mut body: Vec<u8> = vec![];
        io::copy(&mut resp, &mut body).unwrap();
        trace!("{}", String::from_utf8(body).unwrap());
        assert_eq!(resp.status, hyper::status::StatusCode::Created);
        Ok(resp.status)
    }

    pub fn get_project(&self, url: &String) -> Result<Project, DecoderError> {
        json::decode(&self.get_url_string(url).unwrap())
    }

    pub fn get_patch(&self, patch_id: &u64) -> Result<Patch, DecoderError> {
        let url = format!("{}{}/patches/{}{}", &self.url, PATCHWORK_API,
                          patch_id, PATCHWORK_QUERY);
        json::decode(&self.get_url_string(&url).unwrap())
    }

    pub fn get_patch_by_url(&self, url: &String) -> Result<Patch, DecoderError> {
        json::decode(&self.get_url_string(&url).unwrap())
    }

    pub fn get_patch_query(&self) -> Result<Vec<Patch>, DecoderError> {
        let url = format!("{}{}/patches/{}", &self.url, PATCHWORK_API, PATCHWORK_QUERY);
        json::decode(&self.get_url_string(&url).unwrap_or_else(
            |err| panic!("Failed to connect to Patchwork: {}", err)))
    }

    pub fn get_patch_dependencies(&self, patch: &Patch) -> Vec<Patch> {
        // We assume the list of patches in a series are in order.
        let series = self.get_series_by_url(&patch.series[0]).unwrap();
        let mut dependencies: Vec<Patch> = vec!();
        for dependency in series.patches {
            dependencies.push(self.get_patch_by_url(&dependency).unwrap());
            if dependency == patch.url {
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
        let mut mbox = File::create(&path).unwrap_or_else(
            |err| panic!("Couldn't create mbox file: {}", err));
        io::copy(&mut mbox_resp, &mut mbox).unwrap_or_else(
            |err| panic!("Couldn't save mbox from Patchwork: {}", err));
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
            io::copy(&mut mbox_resp, &mut mbox).unwrap_or_else(
            	|err| panic!("Couldn't save mbox from Patchwork: {}", err));
        }
        path
    }

    pub fn get_series(&self, series_id: &u64) -> Result<Series, DecoderError> {
        let url = format!("{}{}/series/{}{}", &self.url, PATCHWORK_API,
                          series_id, PATCHWORK_QUERY);
        json::decode(&self.get_url_string(&url).unwrap())
    }

    pub fn get_series_by_url(&self, url: &String) -> Result<Series, DecoderError> {
        json::decode(&self.get_url_string(&url).unwrap())
    }

}
