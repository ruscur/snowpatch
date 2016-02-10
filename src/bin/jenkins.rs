//
// skibot - continuous integration for patch-based workflows
//
// Copyright (C) 2016 IBM Corporation
// Author: Andrew Donnellan <andrew.donnellan@au1.ibm.com>
//
// This program is free software; you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation; either version 2 of the License, or (at your option)
// any later version.
//
// jenkins.rs - interface to Jenkins REST API
//

extern crate hyper;
extern crate url;

use hyper::Client;
use hyper::header::Location;

fn start_test(jenkins_url: &str, job_name: &str, params: Vec<(&str, &str)>)
              -> Result<String, &'static str> {
    let client = Client::new(); // TODO: do we want to get this from somewhere else?
    let params = url::form_urlencoded::serialize(params);

    let res = client.post(&format!("{}/job/{}/buildWithParameters?{}", jenkins_url, job_name, params)).send().expect("HTTP request error");

    match res.headers.get::<Location>() {
        Some(loc) => Ok(loc.to_string()),
        None => Err("No Location header returned"),
    }
}


// TODO:
// * get Jenkins config details from somewhere
// * get the build number from the build queue entry
// * get status for the build
// * get artifacts + console log from completed build (do we make this configurable?)
// * integrate into skibot worker thread


// Test code only!
fn main() {
    let res = start_test("https://jenkins.ozlabs.ibm.com", "linux-build-manual", vec![("USER_EMAIL", "ajd")]);
    match res {
        Ok(loc) => println!("Location: {}", loc),
        Err(e) => println!("{:?}", e),
    }
}
