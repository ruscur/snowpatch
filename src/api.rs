/*
 * Copyright (c) 2016 Russell Currey <ruscur@russell.cc>
 *
 * This program is free software; you can redistribute it and/or modify it
 * under the terms of the GNU General Public License as published by the Free
 * Software Foundation; either version 2 of the License, or (at your option)
 * any later version.
 */

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

// /api/1.0/series/*/revisions/*/test-results/
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

#[derive(RustcEncodable)]
pub struct TestResult {
    pub test_name: String,
    pub state: String,
    pub url: Option<String>,
    pub summary: Option<String>
}
