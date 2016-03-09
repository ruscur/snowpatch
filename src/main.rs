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

use tempdir::TempDir;

use docopt::Docopt;

use std::io;
use std::fs::File;
use std::string::String;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

mod patchwork;
use patchwork::{PatchworkServer, TestState, TestResult};

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

fn test_patch(patchwork: &PatchworkServer, settings: &Config, project: &Project, series: &patchwork::Result, repo: &Repository) {
    let mut remote = repo.find_remote(&project.remote_name).unwrap();
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_, _, _| {
        return Cred::ssh_key_from_agent("git");
    });
    let mut push_opts = PushOptions::new();
    push_opts.remote_callbacks(callbacks);

    let mut path = TempDir::new("snowpatch").unwrap().into_path();
    let tag = format!("{}-{}-{}", series.submitter.name, series.id, series.version).replace(" ", "_");
    path.push(format!("{}.mbox", tag));

    let mut mbox_resp = patchwork.get_series_mbox(&series.id, &series.version).unwrap();
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
            patchwork.post_test_result(test_result, &series.id, &series.version);
        },
        Err(_) => {
            // It didn't apply.  No need to bother testing.
            test_result = TestResult {
                test_name: "apply_patch".to_string(),
                state: TestState::FAILURE.string(),
                url: None,
                summary: Some("Series failed to apply to branch".to_string()),
            };
            patchwork.post_test_result(test_result, &series.id, &series.version);
            return;
        }
    }

    let settings = settings.clone();
    let project = project.clone();
    let settings_clone = settings.clone();
    // We've set up a remote branch, time to kick off tests
    thread::spawn(move || { run_test(&settings_clone, &project, &tag); }); // TODO: Get result
}

fn main() {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.decode())
        .unwrap_or_else(|e| e.exit());

    let settings = settings::parse(args.arg_config_file);

    // The HTTP client we'll use to access the APIs
    let client = Arc::new(Client::new());

    // Make sure each project repository is starting at the base branch
    for (_, config) in settings.projects.iter() {
        let repo = config.get_repo().unwrap();
        git::checkout_branch(&repo, &config.branch);
    }

    let mut patchwork = PatchworkServer::new(&settings.patchwork.url, &client);
    if settings.patchwork.user.is_some() {
        patchwork.set_authentication(&settings.patchwork.user.clone().unwrap(),
                                     &settings.patchwork.pass.clone());
    }
    let patchwork = patchwork;

    // Poll patchwork for new series. For each series, get patches, apply and test.
    loop {
        let series_list = patchwork.get_series_query().results.unwrap();
        for series in series_list {
            match settings.projects.get(&series.project.name) {
                None => continue,
                Some(project) => {
                    test_patch(&patchwork, &settings, &project, &series, &project.get_repo().unwrap());
                }
            }
        }
        thread::sleep(Duration::new(settings.patchwork.polling_interval, 0));
    }
}
