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

// TODO: every unwrap() needs to be an unwrap_or_else() or similar

extern crate hyper;
extern crate rustc_serialize;
extern crate git2;
extern crate toml;
extern crate tempdir;
extern crate docopt;

use git2::{Cred, Repository, BranchType, RemoteCallbacks, PushOptions};

use hyper::Client;
use hyper::header::Connection;
use hyper::header::{Headers, Accept, ContentType, qitem, Authorization, Basic};
use hyper::mime::{Mime, TopLevel, SubLevel, Attr, Value};

use rustc_serialize::json::{self};

use tempdir::TempDir;

use docopt::Docopt;

use std::io;
use std::fs::File;
use std::str;
use std::string::String;
use std::sync::Arc;
use std::thread;
use std::env;
use std::time::Duration;

mod api;
use api::{Series, TestState, TestResult};

mod jenkins;
use jenkins::{JenkinsBackend, CIBackend, JenkinsBuildStatus};

mod settings;
use settings::{Config, Project};

mod git;
use git::GIT_REF_BASE;

static USAGE: &'static str = "
Usage: snowpatch [options] [<config-file>]

By default, snowpatch runs as a long-running daemon.

Options:
	-n, --count <count>  Run tests on <count> recent series and exit.
	-f, --mbox <mbox>    Run tests on the given mbox file and exit.
	-v, --version        Output version information and exit.
	-h, --help           Output this help text and exit.
";

#[derive(RustcDecodable)]
struct Args {
    arg_config_file: String,
    flag_count: u8,
    flag_mbox: String,
}

// TODO: more constants.  constants for format strings of URLs and such.
static PATCHWORK_API: &'static str = "/api/1.0";
static PATCHWORK_QUERY: &'static str = "?ordering=-last_updated&related=expand";

fn run_test(settings: &Config, project: &Project, tag: &str) {
    let jenkins = JenkinsBackend { base_url: &settings.jenkins.url };
    let project = project.clone();
    for job_params in project.jobs.iter() {
        let job_name = job_params.get("job").unwrap();
        let mut jenkins_params = Vec::<(&str, &str)>::new();
        for (param_name, param_value) in job_params.iter() {
            println!("Param name {}, value {}", &param_name, &param_value);
            match param_name.as_ref() {
                // TODO: Validate special parameter names in config at start of program
                "job" => { },
                "remote" => jenkins_params.push((&param_value, &project.remote_uri)),
                "branch" => jenkins_params.push((&param_value, &tag)),
                _ => jenkins_params.push((&param_name, &param_value)),
            }
        }
        println!("Starting job: {}", &job_name);
        let res = jenkins.start_test(&job_name, jenkins_params).unwrap();
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
}

fn test_patch(settings: &Config, project: &Project, client: &Client, series: &api::Result, headers: &Headers, repo: &Repository) {
    let mut remote = repo.find_remote(&project.remote_name).unwrap();
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_, _, _| {
        return Cred::ssh_key_from_agent("git");
    });
    let mut push_opts = PushOptions::new();
    push_opts.remote_callbacks(callbacks);
    
    let client = client.clone();
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
    let commit = git::get_latest_commit(&repo);
    let mut branch = repo.branch(&tag, &commit, true).unwrap();
    println!("Switching to branch...");
    repo.set_head(branch.get().name().unwrap())
        .unwrap_or_else(|err| panic!("Couldn't set HEAD: {}", err));
    repo.checkout_head(None)
        .unwrap_or_else(|err| panic!("Couldn't checkout HEAD: {}", err));
    println!("Repo is now at head {}", repo.head().unwrap().name().unwrap());

    let output = git::apply_patch(&repo, &path);
    match output {
        Ok(_) => {
            let refspecs: &[&str] = &[&format!("+{}/{}", GIT_REF_BASE, tag)];
            remote.push(refspecs, Some(&mut push_opts)).unwrap();
        }
        _ => {}
    }
    
    git::checkout_branch(&repo, &project.branch);
    // we need to find the branch again since its head has moved
    branch = repo.find_branch(&tag, BranchType::Local).unwrap();
    branch.delete().unwrap();
    println!("Repo is back to {}", repo.head().unwrap().name().unwrap());

    let test_result;
    match output {
        Ok(_) => {
            test_result = TestResult {
                test_name: "apply_patch".to_string(),
                state: TestState::SUCCESS.string(),
                url: None,
                summary: Some("Successfully applied".to_string()),
            };
        },
        Err(e) => {
            // It didn't apply.  No need to bother testing.
            test_result = TestResult {
                test_name: "apply_patch".to_string(),
                state: TestState::FAILURE.string(),
                url: None,
                summary: Some("Series failed to apply to branch".to_string()),
            };
            // TODO: Post...
            //return test_result;
            return;
        }
    }

    let headers = headers.clone();
    let settings = settings.clone();
    let project = project.clone();
    let settings_clone = settings.clone();
    // We've set up a remote branch, time to kick off tests
    thread::spawn(move || { run_test(&settings_clone, &project, &tag); }); // TODO: Get result
    
    // Encode our result into JSON
    let encoded = json::encode(&test_result).unwrap();
    println!("{}", encoded);
    // Send the result to the API
    let res = client.post(&format!("{}{}/series/{}/revisions/{}/test-results/", &settings.patchwork.url, PATCHWORK_API, series.id, series.version)).headers(headers).body(&encoded).send().unwrap();
    assert_eq!(res.status, hyper::status::StatusCode::Created);
}

fn main() {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.decode())
        .unwrap_or_else(|e| e.exit());

    let settings = settings::parse(args.arg_config_file);

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
            // This unwrap is fine since we know it will work
            username: settings.patchwork.user.clone().unwrap(),
            password: settings.patchwork.pass.clone()
        }));
    }
    // Make sure the repository is starting at the base branch
    for (_, config) in settings.projects.iter() {
        let repo = config.get_repo().unwrap();
        git::checkout_branch(&repo, &config.branch);
    }

    loop {
        // Connect to the Patchwork API
        let url = format!("{}{}/series/{}", &settings.patchwork.url, PATCHWORK_API, PATCHWORK_QUERY);
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
        println!("{}", body_str);
        /*
         * For each series, get patches and apply and test...
         * This section is running on the main thread.  The reason for this is
         * all git operations would need to be bound by a mutex anyway, so handle
         * everything before we have a remote with our patches on the main thread.
         */
        for i in 0..results.len() { // TODO: don't use a counter, use a nice for loop
            let settings = settings.clone();
            let client = client.clone();
            let headers = headers.clone();
            let series = results[i].clone();
            // TODO: this is a horrendous way of continuing on fail, fix!
            let project = settings.projects.get(&series.project.name).clone();
            if !project.is_some() {
                continue;
            }
            let settings = settings.clone();
            let project = settings.projects.get(&series.project.name).unwrap().clone();
            let repo = project.get_repo().unwrap();
            let series = results[i].clone();
            test_patch(&settings, &project, &client.clone(), &series, &headers, &repo);
        }
        thread::sleep(Duration::new(settings.patchwork.polling_interval,0));
    }
}
