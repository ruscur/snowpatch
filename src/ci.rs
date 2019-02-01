//
// snowpatch - continuous integration for patch-based workflows
//
// Copyright (C) 2016-2019 IBM Corporation
// Authors:
//     Russell Currey <ruscur@russell.cc>
//     Andrew Donnellan <andrew.donnellan@au1.ibm.com>
//
// This program is free software; you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation; either version 2 of the License, or (at your option)
// any later version.
//
// ci.rs - CI backend interface definitions
//

use std::collections::BTreeMap;
use std::error::Error;

use patchwork::TestState;

#[derive(Eq, PartialEq)]
pub enum BuildStatus {
    Running,
    Done,
}

pub trait CIBackend {
    fn start_test(&self, job_name: &str, params: Vec<(&str, &str)>)
        -> Result<String, &'static str>;
    fn get_build_handle(&self, build_queue_entry: &str) -> Result<String, Box<Error>>;
    fn get_build_result(&self, build_handle: &str) -> Result<TestState, Box<Error>>;
    fn get_results_url(&self, build_handle: &str, job: &BTreeMap<String, String>) -> String;
    fn get_description(&self, build_handle: &str, job: &BTreeMap<String, String>)
        -> Option<String>;
    fn wait_build(&self, build_handle: &str) -> Result<BuildStatus, Box<Error>>;
}
