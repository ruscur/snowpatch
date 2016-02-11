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
// bin/jenkins_test.rs - test program for Jenkins REST API
//

extern crate hyper;
extern crate url;
extern crate rustc_serialize;

extern crate snowpatch;

use std::thread::sleep;
use std::time::Duration;

use snowpatch::jenkins::{JenkinsBackend, CIBackend, JenkinsBuildStatus};

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
            JenkinsBuildStatus::Running => {},
            JenkinsBuildStatus::Done => {println!{"DONE!"}; break},
        }
        
    }   
}
