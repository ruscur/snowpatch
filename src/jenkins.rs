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
// jenkins.rs - interface to Jenkins REST API
//

// TODO:
// * get Jenkins config details from somewhere
// * get status for the build
// * get artifacts + console log from completed build (do we make this configurable?)
// * integrate into snowpatch worker thread

extern crate base64;
extern crate reqwest;
extern crate url;

use std::collections::BTreeMap;
use std::error::Error;
use std::io::Read;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use reqwest::header::{HeaderMap, AUTHORIZATION, LOCATION};
use reqwest::{Client, IntoUrl, Response};
use serde_json::{self, Value};

use ci::{BuildStatus, CIBackend};

use patchwork::TestState;

// Constants
const JENKINS_POLLING_INTERVAL: u64 = 5000; // Polling interval in milliseconds

// Jenkins API definitions

pub struct JenkinsBackend {
    pub base_url: String,
    pub reqwest_client: Arc<Client>,
    pub username: Option<String>,
    pub token: Option<String>,
}

impl CIBackend for JenkinsBackend {
    /// Start a Jenkins build
    ///
    /// # Failures
    ///
    /// Returns Err when HTTP request fails or when no Location: header is returned
    fn start_test(
        &self,
        job_name: &str,
        params: Vec<(&str, &str)>,
    ) -> Result<String, &'static str> {
        let params = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(params)
            .finish();

        let resp = self
            .post_url(&format!(
                "{}/job/{}/buildWithParameters?{}",
                self.base_url, job_name, params
            ))
            .expect("HTTP request error"); // TODO don't panic here

        match resp.headers().get(LOCATION) {
            // TODO do we actually have to return a string, coud we change the API?
            Some(loc) => Ok(loc.to_str().unwrap().to_string()),
            None => Err("No Location header returned"),
        }
    }
}

impl JenkinsBackend {
    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(ref username) = self.username {
            if let Some(ref token) = self.token {
                headers.insert(
                    AUTHORIZATION,
                    format!(
                        "Basic {}",
                        base64::encode(&format!("{}:{}", username, token))
                    )
                    .parse()
                    .unwrap(),
                );
            }
        };
        headers
    }

    fn get_url<U: IntoUrl>(&self, url: U) -> Result<Response, reqwest::Error> {
        self.reqwest_client.get(url).headers(self.headers()).send()
    }

    fn post_url<U: IntoUrl>(&self, url: U) -> Result<Response, reqwest::Error> {
        self.reqwest_client.post(url).headers(self.headers()).send()
    }

    fn get_api_json_object(&self, base_url: &str) -> Result<Value, Box<Error>> {
        let url = format!("{}api/json", base_url);
        let mut result_str = String::new();
        loop {
            let mut resp = match self.get_url(&url) {
                Ok(r) => r,
                Err(e) => {
                    // TODO: We have to time out rather than spinning indefinitely
                    warn!("Couldn't hit Jenkins API: {}", e);
                    sleep(Duration::from_millis(JENKINS_POLLING_INTERVAL));
                    continue;
                }
            };

            if resp.status().is_server_error() {
                // TODO: Timeout
                sleep(Duration::from_millis(JENKINS_POLLING_INTERVAL));
                continue;
            }
            resp.read_to_string(&mut result_str)
                .map_err(|e| format!("Couldn't read from server: {}", e))?;
            break;
        }
        serde_json::from_str(&result_str)
            .map_err(|e| format!("Couldn't parse JSON from Jenkins: {}", e).into())
    }

    pub fn get_build_handle(&self, build_queue_entry: &str) -> Result<String, Box<Error>> {
        loop {
            let entry = self.get_api_json_object(build_queue_entry)?;
            match entry.get("executable") {
                Some(exec) => {
                    let url = exec
                        .as_object() // Option<BTreeMap>
                        .unwrap() // BTreeMap
                        .get("url") // Option<&str>
                        .unwrap() // &str ?
                        .as_str()
                        .unwrap()
                        .to_string();
                    return Ok(url);
                }
                // TODO: Timeout / handle this case properly
                None => sleep(Duration::from_millis(JENKINS_POLLING_INTERVAL)),
            }
        }
    }

    pub fn get_build_status(&self, build_handle: &str) -> Result<BuildStatus, Box<Error>> {
        match self.get_api_json_object(build_handle)?["building"].as_bool() {
            Some(true) => Ok(BuildStatus::Running),
            Some(false) => Ok(BuildStatus::Done),
            None => Err("Error getting build status".into()),
        }
    }

    pub fn get_build_result(&self, build_handle: &str) -> Result<TestState, Box<Error>> {
        match self
            .get_api_json_object(build_handle)?
            .get("result")
            .map(|v| v.as_str().unwrap_or("PENDING"))
        {
            None => Ok(TestState::Pending),
            Some(result) => match result {
                // TODO: Improve this...
                "SUCCESS" => Ok(TestState::Success),
                "FAILURE" => Ok(TestState::Fail),
                "UNSTABLE" => Ok(TestState::Warning),
                _ => Ok(TestState::Pending),
            },
        }
    }

    pub fn get_results_url(&self, build_handle: &str, job: &BTreeMap<String, String>) -> String {
        let default_url = format!("{}/", build_handle);
        match job.get("artifact") {
            Some(artifact) => {
                let artifact_url = format!("{}/artifact/{}", build_handle, artifact);
                match self.get_url(&artifact_url) {
                    Ok(mut resp) => match resp.status().is_success() {
                        true => artifact_url,
                        false => default_url,
                    },
                    Err(_e) => default_url,
                }
            }
            None => default_url,
        }
    }

    pub fn get_description(
        &self,
        build_handle: &str,
        job: &BTreeMap<String, String>,
    ) -> Option<String> {
        match job.get("description") {
            Some(artifact) => {
                match self.get_url(&format!("{}/artifact/{}", build_handle, artifact)) {
                    Ok(mut resp) => match resp.status().is_success() {
                        true => match resp.text() {
                            Ok(text) => Some(text),
                            Err(_e) => None,
                        },
                        false => None,
                    },
                    Err(_e) => None,
                }
            }
            None => None,
        }
    }

    pub fn wait_build(&self, build_handle: &str) -> Result<BuildStatus, Box<Error>> {
        // TODO: Implement a timeout?
        while self.get_build_status(build_handle)? != BuildStatus::Done {
            sleep(Duration::from_millis(JENKINS_POLLING_INTERVAL));
        }
        Ok(BuildStatus::Done)
    }
}
