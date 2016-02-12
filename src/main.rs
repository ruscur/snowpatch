//
// snowpatch - continuous integration for patch-based workflows
//
// Copyright (C) 2016 IBM Corporation
// Author: Russell Currey <ruscur@russell.cc>
//
// This program is free software; you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation; either version 2 of the License, or (at your option)
// any later version.
//
// main.rs - snowpatch main program
//

extern crate hyper;
extern crate rustc_serialize;
extern crate git2;
extern crate toml;
extern crate tempdir;

use git2::{Cred, Repository, BranchType, RemoteCallbacks, PushOptions};
use git2::build::CheckoutBuilder;

use hyper::Client;
use hyper::header::Connection;
use hyper::header::{Headers, Accept, ContentType, qitem, Authorization, Basic};
use hyper::mime::{Mime, TopLevel, SubLevel, Attr, Value};

use rustc_serialize::json::{self};

use std::io;
use std::fs::File;
use std::str;
use std::process::Command;
use std::string::String;
use std::sync::Arc;
use std::thread;
use std::env;
use tempdir::TempDir;

pub mod api;
use api::{Series, TestState, TestResult};

pub mod jenkins;
use jenkins::{JenkinsBackend, CIBackend, JenkinsBuildStatus};

mod settings;

const PATCHWORK_API: &'static str = "/api/1.0";
const PATCHWORK_QUERY: &'static str = "?ordering=-last_updated&related=expand";
const GIT_REF_BASE: &'static str = "refs/heads";

fn main() {
    let settings = settings::parse(env::args().nth(1).unwrap());
    /*
     * Eventually, the main loop will be polling Patchwork for new revisions,
     * instead of iterating through all recent ones.  At that point, it will
     * be able to handle multiple projects.  For now, however, just handle
     * recent patches of the project passed by the command line.
     */
    let project_name = env::args().nth(2).unwrap();
    let project = settings.projects.get(&project_name).unwrap();

    let repo = Repository::open(&project.repository).unwrap();

    // The HTTP client we'll use to access the APIs
    let client_base = Arc::new(Client::new());
    let client = client_base.clone();

    let mut headers = Headers::new();
    headers.set(Accept(vec![qitem(Mime(TopLevel::Application,
                                       SubLevel::Json,
                                       vec![(Attr::Charset, Value::Utf8)]))])
    );
    headers.set(ContentType(Mime(TopLevel::Application,
                                 SubLevel::Json,
                                 vec![(Attr::Charset, Value::Utf8)]))
    );

    if settings.patchwork.user.is_some() {
        headers.set(Authorization(Basic {
            username: settings.patchwork.user.clone().unwrap(),
            password: settings.patchwork.pass.clone()
        }));
    }
    // Make sure the repository is starting at master
    repo.set_head(&format!("{}/{}", GIT_REF_BASE, &project.branch))
        .unwrap_or_else(|err| panic!("Couldn't set HEAD: {}", err));
    repo.checkout_head(Some(&mut CheckoutBuilder::new().force()))
        .unwrap_or_else(|err| panic!("Couldn't checkout HEAD: {}", err));

    // Set up the remote, and its related authentication
    let mut remote = repo.find_remote(&project.remote).unwrap();
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_, _, _| {
        return Cred::ssh_key_from_agent("git");
    });
    let mut push_opts = PushOptions::new();
    push_opts.remote_callbacks(callbacks);

    // Find the latest commit, which we'll use to branch
    let head = repo.head().unwrap();
    let oid = head.target().unwrap();
    let commit = repo.find_commit(oid).unwrap();

    // Connect to the Patchwork API
    let url = format!("{}{}/projects/{}/series/{}", &settings.patchwork.url, PATCHWORK_API, project_name, PATCHWORK_QUERY);
    let mut resp = client.get(&*url).headers(headers.clone()).header(Connection::close()).send().unwrap();
    // Copy the body into our buffer
    let mut body: Vec<u8> = vec![];
    io::copy(&mut resp, &mut body).unwrap();
    // Convert the body into a string so we can decode it
    let body_str = str::from_utf8(&body).unwrap();
    // Decode the json string into our Series struct
    let decoded: Series = json::decode(body_str).unwrap();
    // Get our results: the list of patch series the API gave us
    let results = decoded.results.unwrap();

    /*
     * For each series, get patches and apply and test...
     * This will eventually be refactored into a poller.
     * For now, we need to be able to quickly re-test the whole process.
     * This section is running on the main thread.  The reason for this is
     * all git operations would need to be bound by a mutex anyway, so handle
     * everything before we have a remote with our patches on the main thread.
     */
    for i in 0..results.len() {
        let client = client.clone();
        let series = results[i].clone();
        let mut path = TempDir::new("snowpatch").unwrap().into_path();
        let tag = format!("{}-{}-{}", series.submitter.name, series.id, series.version).replace(" ", "_");
        path.push(format!("{}.mbox", tag));
        let mbox_url = format!("{}{}/series/{}/revisions/{}/mbox/", &settings.patchwork.url, PATCHWORK_API, series.id, series.version);
        let mut mbox_resp = client.get(&*mbox_url).headers(headers.clone()).header(Connection::close()).send().unwrap();
        println!("Opening file {}", path.display());
        let mut mbox = File::create(&path).unwrap();
        println!("Writing to file...");
        io::copy(&mut mbox_resp, &mut mbox).unwrap();
        println!("Creating a new branch...");
        let mut branch = repo.branch(&tag, &commit, true).unwrap();
        println!("Switching to branch...");
        repo.set_head(branch.get().name().unwrap())
            .unwrap_or_else(|err| panic!("Couldn't set HEAD: {}", err));
        repo.checkout_head(None)
            .unwrap_or_else(|err| panic!("Couldn't checkout HEAD: {}", err));
        println!("Repo is now at head {}", repo.head().unwrap().name().unwrap());

        let output = Command::new("git") // no "am" support in libgit2
            .arg("am") // apply from mbox
            .arg(&path) // ...from our file
            .current_dir(&project.repository) // ...in the repo
            .output() // ...and synchronously run it now
            .unwrap(); // ...and we'll assume there's no issue with that

        if output.status.success() {
            println!("Patch applied with text {}", String::from_utf8(output.clone().stdout).unwrap());
            // push the new branch to the remote
            let refspecs: &[&str] = &[&format!("+{}/{}", GIT_REF_BASE, tag)];
            remote.push(refspecs, Some(&mut push_opts)).unwrap();
        } else {
            Command::new("git").arg("am").arg("--abort").current_dir(&project.repository).output().unwrap();
            println!("Patch did not apply successfully");
        }
        repo.set_head(&format!("{}/{}", GIT_REF_BASE, &project.branch))
            .unwrap_or_else(|err| panic!("Couldn't set HEAD: {}", err));
        repo.checkout_head(Some(&mut CheckoutBuilder::new().force()))
            .unwrap_or_else(|err| panic!("Couldn't checkout HEAD: {}", err));
        // we need to find the branch again since its head has moved
        branch = repo.find_branch(&tag, BranchType::Local).unwrap();
        branch.delete().unwrap();
        println!("Repo is back to {}", repo.head().unwrap().name().unwrap());
        let headers = headers.clone();
        let settings = settings.clone();
        let project = project.clone();

        // We've set up a remote branch, time to kick off tests
        thread::spawn(move || {
            let test_result;
            if !output.status.success() {
                // It didn't apply.  No need to bother testing.
                test_result = TestResult {
                    test_name: "apply_patch".to_string(),
                    state: TestState::FAILURE.string(),
                    url: None,
                    summary: Some("Series failed to apply to branch master".to_string()),
                };
            } else {
                test_result = TestResult {
                    test_name: "apply_patch".to_string(),
                    state: TestState::SUCCESS.string(),
                    url: None,
                    summary: Some("Successfully applied".to_string()),
                };
            }

            // Spawn a jenkins job
            let jenkins = JenkinsBackend { base_url: &settings.jenkins.url };
            for job in project.jobs {
                println!("Starting job: {}", &job);
                let res = jenkins.start_test(&job, vec![("USER_EMAIL", "ajd")]).unwrap();
                println!("{:?}", &res);
                let build_url_real;
                loop {
                    let build_url = jenkins.get_build_url(&res);
                    match build_url {
                        Some(url) => { build_url_real = url; break; },
                        None => { },
                    }
                }
                println!("Build URL: {}", build_url_real);

                loop {
                    let status = jenkins.get_build_status(&build_url_real);
                    match status  {
                        JenkinsBuildStatus::Done => break,
                        _ => {}
                    }
                }
                println!("Job done!");
            }

            // Encode our result into JSON
            let encoded = json::encode(&test_result).unwrap();
            println!("{}", encoded);
            // Send the result to the API
            let res = client.post(&format!("{}{}/series/{}/revisions/{}/test-results/", &settings.patchwork.url, PATCHWORK_API, series.id, series.version)).headers(headers).body(&encoded).send().unwrap();
            assert_eq!(res.status, hyper::status::StatusCode::Created);
            
        });
    }
}
