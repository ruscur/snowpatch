//
// snowpatch - continuous integration for patch-based workflows
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


// TODO:
// * get Jenkins config details from somewhere
// * get status for the build
// * get artifacts + console log from completed build (do we make this configurable?)
// * integrate into snowpatch worker thread

extern crate hyper;
extern crate url;
extern crate rustc_serialize;

use std::io::Read;
use std::thread::sleep;
use std::time::Duration;

use hyper::Client;
use hyper::header::Location;
use rustc_serialize::json::Json;





use std::io;

// Jenkins API definitions


trait CIBackend {
    fn start_test(&self, job_name: &str, params: Vec<(&str, &str)>) -> Result<String, &'static str>;
}


struct JenkinsBackend<'a> {
    base_url: &'a str,
    // TODO: Authentication
}



impl<'a> CIBackend for JenkinsBackend<'a> {
    /// Start a Jenkins build
    ///
    /// # Failures
    ///
    /// Returns Err when HTTP request fails or when no Location: header is returned
    fn start_test(&self, job_name: &str, params: Vec<(&str, &str)>)
                  -> Result<String, &'static str> {
        let client = Client::new(); // TODO: do we want to get this from somewhere else?
        let params = url::form_urlencoded::serialize(params);
        
        let res = client.post(&format!("{}/job/{}/buildWithParameters?{}", self.base_url, job_name, params)).send().expect("HTTP request error"); // TODO don't panic here
        
        match res.headers.get::<Location>() {
            Some(loc) => Ok(loc.to_string()),
            None => Err("No Location header returned"),
        }
    }
}

enum JenkinsBuildStatus {
    Running,
    Done,
}

use JenkinsBuildStatus::{Running,Done};

impl<'a> JenkinsBackend<'a> {
    fn get_build_url(&self, build_queue_entry: &str) -> Option<String> {
        let client = Client::new(); // TODO
        let url = format!("{}api/json", build_queue_entry);
        
        let mut res = client.get(&url).send().expect("HTTP request error"); // TODO don't panic here
        let mut result_str = String::new();
        res.read_to_string(&mut result_str);
        let json = Json::from_str(&result_str).unwrap();
        let obj = json.as_object().unwrap();

        match obj.get("executable") {
            Some(exec) => Some(exec.as_object().unwrap().get("url").unwrap().as_string().unwrap().to_string()),
            None => None
        }
    }

    fn get_build_status(&self, build_url: &str) -> JenkinsBuildStatus {
        let client = Client::new();
        let url = format!("{}api/json", build_url);
        let mut res = client.get(&url).send().expect("HTTP request error");
        let mut result_str = String::new();
        res.read_to_string(&mut result_str);
        let json = Json::from_str(&result_str).unwrap();
        //println!("build status json: {:?}", json);
        match json.as_object().unwrap().get("building").unwrap().as_boolean().unwrap() {
            true => Running,
            false => Done,
        }
    }
}
    
    

// Test code only!
fn main() {
    let jenkins = JenkinsBackend { base_url: "https://jenkins.ozlabs.ibm.com" };
    let res = jenkins.start_test("linux-build-manual", vec![("USER_EMAIL", "ajd")]);
    let mut build_queue_location = String::new();
    
    match res {
        Ok(loc) => {
            println!("Location: {}", loc);
            build_queue_location = loc;
        }
            
        Err(e) => {
            println!("{:?}", e);
            return;
        }
    }

    let build_url;
    println!("Polling until we have a build executable location...");
    loop {
        sleep(Duration::new(1,0));
        let build_url_res = jenkins.get_build_url(&build_queue_location);
        println!("Build URL: {:?}", build_url_res);
        match build_url_res {
            Some(url) => { build_url = url; break; },
            None => {},
        }
    }

    println!("Polling until job completion...");
    loop {
        sleep(Duration::new(1,0));
        let build_status = jenkins.get_build_status(&build_url);
        match build_status {
            Running => {},
            Done => {println!{"DONE!"}; break},
        }
        
    }
    
    
    
}
