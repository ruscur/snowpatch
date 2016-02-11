/*
 * Copyright (c) 2016 IBM Corporation
 * Author: Russell Currey <ruscur@russell.cc>
 *
 * This program is free software; you can redistribute it and/or modify it
 * under the terms of the GNU General Public License as published by the Free
 * Software Foundation; either version 2 of the License, or (at your option)
 * any later version.
 */

extern crate hyper;
extern crate rustc_serialize;
extern crate git2;
extern crate toml;

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
use std::path::Path;
use std::process::Command;
use std::string::String;
use std::sync::Arc;
use std::thread;
use std::env;

pub mod api;
use api::{Series, TestState, TestResult};

pub mod jenkins;

mod settings;
use settings::Config;

fn main() {
    // TODO: refactor these into passable arguments
    let settings = settings::parse(env::args().nth(1).unwrap());
    let api_base = "https://russell.cc/patchwork/api/1.0";
    let api_query = "?ordering=-last_updated&related=expand";
    let project_name = "linuxppc-dev";
    let base_branch = "refs/heads/master";
    let remote_name = "github";
    let repo_path = "/home/ruscur/Documents/linux/";

    let repo = Repository::open(repo_path).unwrap();
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
    headers.set(Authorization(Basic {
        username: "ruscur".to_string(),
        password: Some("banana".to_string())
    }));

    // Make sure the repository is starting at master
    repo.set_head(base_branch);
    repo.checkout_head(Some(&mut CheckoutBuilder::new().force()));

    // Set up the remote, and its related authentication
    let mut remote = repo.find_remote(remote_name).unwrap();
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_, _, _| {
        return Cred::ssh_key_from_agent("git");
    });
    let mut push_opts = PushOptions::new();
    push_opts.remote_callbacks(callbacks);

    // Find the latest commit, which we'll use to branch
    let head = repo.head().unwrap();
    let oid = head.target().unwrap();
    let mut commit = repo.find_commit(oid).unwrap();
    println!("Commit: {}", commit.summary().unwrap());

    // Connect to the Patchwork API
    let url = format!("{}/projects/{}/series/{}", api_base, project_name, api_query);
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
        let tag = format!("{}-{}-{}", series.submitter.name, series.id, series.version).replace(" ", "_");
        let mbox_path = format!("/tmp/patches/{}.mbox", tag);
        let mbox_url = format!("{}/series/{}/revisions/{}/mbox/", api_base, series.id, series.version);
        let mut mbox_resp = client.get(&*mbox_url).headers(headers.clone()).header(Connection::close()).send().unwrap();
        let path = Path::new(&mbox_path);
        println!("Opening file {}", path.display());
        let mut mbox = File::create(&path).unwrap();
        println!("Writing to file...");
        io::copy(&mut mbox_resp, &mut mbox).unwrap();
        println!("Creating a new branch...");
        let mut branch = repo.branch(&tag, &commit, true).unwrap();
        println!("Switching to branch...");
        repo.set_head(branch.get().name().unwrap());
        repo.checkout_head(None);
        println!("Repo is now at head {}", repo.head().unwrap().name().unwrap());

        let output = Command::new("git") // no "am" support in libgit2
            .arg("am") // apply from mbox
            .arg(&mbox_path) // ...from our file
            .current_dir(repo_path) // ...in the repo
            .output() // ...and synchronously run it now
            .unwrap(); // ...and we'll assume there's no issue with that

        if output.status.success() {
            println!("Patch applied with text {}", String::from_utf8(output.clone().stdout).unwrap());
            // push the new branch to the remote
            let refspecs: &[&str] = &[&format!("+refs/heads/{}", tag)];
            remote.push(refspecs, Some(&mut push_opts)).unwrap();
        } else {
            Command::new("git").arg("am").arg("--abort").current_dir(repo_path).output().unwrap();
            println!("Patch did not apply successfully");
        }

        repo.set_head(base_branch);
        repo.checkout_head(Some(&mut CheckoutBuilder::new().force()));
        // we need to find the branch again since its head has moved
        branch = repo.find_branch(&tag, BranchType::Local).unwrap();
        branch.delete().unwrap();
        println!("Repo is back to {}", repo.head().unwrap().name().unwrap());
        let headers = headers.clone();

        // We've set up a remote branch, time to kick off tests
        thread::spawn(move || {
            if !output.status.success() {
                // It didn't apply.  No need to bother testing.
                let test_result = TestResult {
                    test_name: "apply_patch".to_string(),
                    state: TestState::FAILURE.string(),
                    url: None,
                    summary: Some("Series failed to apply to branch master".to_string()),
                };
                // Encode our result into JSON
                let encoded = json::encode(&test_result).unwrap();
                println!("{}", encoded);
                // Send the result to the API
                let res = client.post(&format!("{}/series/{}/revisions/{}/test-results/", api_base, series.id, series.version)).headers(headers).body(&encoded).send().unwrap();
                assert_eq!(res.status, hyper::status::StatusCode::Created);
            }
        });
    }
}
