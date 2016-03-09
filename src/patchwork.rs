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
use std::str;
use std::option::Option;

extern crate hyper;
use hyper::Client;
use hyper::header::{Connection, Headers, Accept, ContentType, qitem, Authorization, Basic};
use hyper::mime::{Mime, TopLevel, SubLevel, Attr, Value};
use hyper::status::StatusCode;
use hyper::client::response::Response;

use rustc_serialize::json::{self};

// TODO: more constants.  constants for format strings of URLs and such.
pub static PATCHWORK_API: &'static str = "/api/1.0";
pub static PATCHWORK_QUERY: &'static str = "?ordering=-last_updated&related=expand";

// /api/1.0/projects/*/series/

#[derive(RustcDecodable, Clone)]
pub struct Project {
    pub id: u64,
    pub name: String,
    pub linkname: String,
    pub listemail: String,
    pub web_url: Option<String>,
    pub scm_url: Option<String>,
    pub webscm_url: Option<String>
}

#[derive(RustcDecodable, Clone)]
pub struct Submitter {
    pub id: u64,
    pub name: String
}

#[derive(RustcDecodable, Clone)]
pub struct Result {
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

#[derive(RustcDecodable)]
pub struct Series {
    pub count: u64,
    pub next: Option<String>,
    pub previous: Option<String>,
    pub results: Option<Vec<Result>>
}

pub enum TestState {
    PENDING,
    SUCCESS,
    WARNING,
    FAILURE
}

impl TestState {
    pub fn string(&self) -> String {
        match *self {
            TestState::PENDING => "pending".to_string(),
            TestState::SUCCESS => "success".to_string(),
            TestState::WARNING => "warning".to_string(),
            TestState::FAILURE => "failure".to_string(),
        }
    }
}

// /api/1.0/series/*/revisions/*/test-results/
#[derive(RustcEncodable)]
pub struct TestResult {
    pub test_name: String,
    pub state: String,
    pub url: Option<String>,
    pub summary: Option<String>
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

    pub fn post_test_result(&self, result: TestResult, series_id: &u64, series_version: &u64) -> std::result::Result<StatusCode, hyper::error::Error> {
        let encoded = json::encode(&result).unwrap();
        let headers = self.headers.clone();
        println!("JSON Encoded: {}", encoded);
        let res = try!(self.client.post(&format!("{}{}/series/{}/revisions/{}/test-results/", &self.url, PATCHWORK_API, &series_id, &series_version)).headers(headers).body(&encoded).send());
        assert_eq!(res.status, hyper::status::StatusCode::Created);
        return Ok(res.status);
    }

    pub fn get_series_mbox(&self, series_id: &u64, series_version: &u64) -> std::result::Result<Response, hyper::error::Error> {
        let mbox_url = format!("{}{}/series/{}/revisions/{}/mbox/", &self.url, PATCHWORK_API, series_id, series_version);
        self.client.get(&*mbox_url).headers(self.headers.clone()).header(Connection::close()).send()
    }

    pub fn get_series_query(&self) -> Series {
        let url = format!("{}{}/series/{}", &self.url, PATCHWORK_API, PATCHWORK_QUERY);
        let mut resp = self.client.get(&*url).headers(self.headers.clone()).header(Connection::close()).send().unwrap();
        // Copy the body into our buffer
        let mut body: Vec<u8> = vec![];
        io::copy(&mut resp, &mut body).unwrap();
        // Convert the body into a string so we can decode it
        let body_str = str::from_utf8(&body).unwrap();
        // Decode the json string into our Series struct
        json::decode(body_str).unwrap()
    }
}
