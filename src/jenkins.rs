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

extern crate hyper;
extern crate url;
extern crate rustc_serialize;

use std::io::Read;
use std::time::Duration;
use std::thread::sleep;
use std::sync::Arc;

use hyper::Client;
use hyper::header::Location;
use rustc_serialize::json::Json;

// Constants
const JENKINS_POLLING_INTERVAL: u64 = 5000; // Polling interval in milliseconds

// Jenkins API definitions

pub trait CIBackend { // TODO: Separate out
    fn start_test(&self, job_name: &str, params: Vec<(&str, &str)>) -> Result<String, &'static str>;
}

pub struct JenkinsBackend {
    pub base_url: String,
    pub hyper_client: Arc<Client>,
    // TODO: Authentication
}

impl CIBackend for JenkinsBackend {
    /// Start a Jenkins build
    ///
    /// # Failures
    ///
    /// Returns Err when HTTP request fails or when no Location: header is returned
    fn start_test(&self, job_name: &str, params: Vec<(&str, &str)>)
                  -> Result<String, &'static str> {
        let params = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(params)
            .finish();

        let res = self.hyper_client
            .post(&format!("{}/job/{}/buildWithParameters?{}",
                           self.base_url, job_name, params))
            .send().expect("HTTP request error"); // TODO don't panic here

        match res.headers.get::<Location>() {
            Some(loc) => Ok(loc.to_string()),
            None => Err("No Location header returned"),
        }
    }
}

pub enum JenkinsBuildStatus {
    Running,
    Done,
}

impl JenkinsBackend {
    fn get_api_json_object(&self, base_url: &str) -> rustc_serialize::json::Object {
        // TODO: Don't panic on failure, fail more gracefully
        let url = format!("{}api/json", base_url);
        let mut resp = self.hyper_client.get(&url).send().expect("HTTP request error");
        let mut result_str = String::new();
        resp.read_to_string(&mut result_str)
            .unwrap_or_else(|err| panic!("Couldn't read from server: {}", err));
        let json = Json::from_str(&result_str).unwrap_or_else(|err| panic!("Couldn't parse JSON from Jenkins: {}", err));
        json.as_object().unwrap().clone()
    }

    pub fn get_build_url(&self, build_queue_entry: &str) -> Option<String> {
        match self.get_api_json_object(build_queue_entry).get("executable") {
            Some(exec) => Some(exec.as_object().unwrap().get("url").unwrap().as_string().unwrap().to_string()),
            None => None
        }
    }

    pub fn get_build_status(&self, build_url: &str) -> JenkinsBuildStatus {
        match self.get_api_json_object(build_url).get("building").unwrap().as_boolean().unwrap() {
            true => JenkinsBuildStatus::Running,
            false => JenkinsBuildStatus::Done,
        }
    }

    pub fn wait_build(&self, build_url: &str) -> JenkinsBuildStatus {
        // TODO: Implement a timeout?
        loop {
            match self.get_build_status(&build_url) {
                JenkinsBuildStatus::Done => return JenkinsBuildStatus::Done,
                _ => { },
            }
            sleep(Duration::from_millis(JENKINS_POLLING_INTERVAL));
        }
    }
}
