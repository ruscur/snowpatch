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

// Equivalent of -Werror
#![deny(warnings)]

extern crate hyper;
extern crate rustc_serialize;
extern crate git2;
extern crate toml;
extern crate tempdir;
extern crate docopt;

use git2::{Cred, BranchType, RemoteCallbacks, PushOptions};

use hyper::Client;

use docopt::Docopt;

use std::fs;
use std::string::String;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::path::Path;

mod patchwork;
use patchwork::{PatchworkServer, TestState, TestResult};

mod jenkins;
use jenkins::{JenkinsBackend, CIBackend};

mod settings;
use settings::{Config, Project};

mod git;

mod utils;

static USAGE: &'static str = "
Usage:
  snowpatch <config-file> [--count=<count> | --series <id>]
  snowpatch <config-file> --mbox=<mbox> --project=<name>
  snowpatch -v | --version
  snowpatch -h | --help

By default, snowpatch runs as a long-running daemon.

Options:
  --count <count>           Run tests on <count> recent series.
  --series <id>             Run tests on the given Patchwork series.
  --mbox <mbox>             Run tests on the given mbox file...
  --project <name>          ...as if it were sent to project <name>.
  -v, --version             Output version information.
  -h, --help                Output this help text.
";

#[derive(RustcDecodable)]
struct Args {
    arg_config_file: String,
    flag_count: u16,
    flag_series: u32,
    flag_mbox: String,
    flag_project: String,
}

fn run_tests(settings: &Config, project: &Project, tag: &str) -> Vec<TestResult> {
    let mut results: Vec<TestResult> = Vec::new();
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
        let res = jenkins.start_test(&job_name, jenkins_params)
            .unwrap_or_else(|err| panic!("Starting Jenkins test failed: {}", err));
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
        jenkins.wait_build(&build_url_real);
        println!("Job done!");
        results.push(TestResult {
            test_name: job_name.to_string(),
            state: TestState::SUCCESS.string(), // TODO: get this from Jenkins
            url: None, // TODO: link to Jenkins job log
            summary: Some("TODO: get this summary from Jenkins".to_string()),
        });
    }
    results
}

fn test_patch(settings: &Config, project: &Project, path: &Path) -> Vec<TestResult> {
    let repo = project.get_repo().unwrap();
    let mut results: Vec<TestResult> = Vec::new();
    if !path.is_file() {
        return results;
    }
    let tag = utils::sanitise_path(
        path.file_name().unwrap().to_str().unwrap().to_string());
    let mut remote = repo.find_remote(&project.remote_name).unwrap();
    let commit = git::get_latest_commit(&repo);

    let mut push_callbacks = RemoteCallbacks::new();
    push_callbacks.credentials(|_, _, _| {
        return Cred::ssh_key_from_agent("git");
    });

    let mut push_opts = PushOptions::new();
    push_opts.remote_callbacks(push_callbacks);

    let mut successfully_applied = false;
    for branch_name in project.branches.clone() {
        let tag = format!("{}_{}", tag, branch_name);
        println!("Switching to base branch {}...", branch_name);
        git::checkout_branch(&repo, &branch_name);

        // Make sure we're up to date
        git::pull(&repo).unwrap();

        println!("Creating a new branch...");
        let mut branch = repo.branch(&tag, &commit, true).unwrap();
        println!("Switching to branch...");
        repo.set_head(branch.get().name().unwrap())
            .unwrap_or_else(|err| panic!("Couldn't set HEAD: {}", err));
        repo.checkout_head(None)
            .unwrap_or_else(|err| panic!("Couldn't checkout HEAD: {}", err));
        println!("Repo is now at head {}", repo.head().unwrap().name().unwrap());

        let output = git::apply_patch(&repo, &path);

        match output {
            Ok(_) => git::push_to_remote(
                &mut remote, &tag, &mut push_opts).unwrap(),
            _ => {}
        }

        git::checkout_branch(&repo, &branch_name);
        // we need to find the branch again since its head has moved
        branch = repo.find_branch(&tag, BranchType::Local).unwrap();
        branch.delete().unwrap();
        println!("Repo is back to {}", repo.head().unwrap().name().unwrap());

        match output {
            Ok(_) => {
                successfully_applied = true;
                results.push(TestResult {
                    test_name: "apply_patch".to_string(),
                    state: TestState::SUCCESS.string(),
                    url: None,
                    summary: Some(format!("Successfully applied to branch {}", branch_name)),
                });
            },
            Err(_) => {
                // It didn't apply.  No need to bother testing.
                results.push(TestResult {
                    test_name: "apply_patch".to_string(),
                    state: TestState::WARNING.string(),
                    url: None,
                    summary: Some(format!("Failed to apply to branch {}", branch_name)),
                });
                continue;
            }
        }

        let settings = settings.clone();
        let project = project.clone();
        let settings_clone = settings.clone();
        let test_all_branches = project.test_all_branches.unwrap_or(true);

        // We've set up a remote branch, time to kick off tests
        let test = thread::Builder::new().name(tag.to_string()).spawn(move || {
            return run_tests(&settings_clone, &project, &tag);
        }).unwrap();
        results.append(&mut test.join().unwrap());

        if !test_all_branches { break; }
    }

    if !successfully_applied {
        results.push(TestResult {
            test_name: "apply_patch".to_string(),
            state: TestState::FAILURE.string(),
            url: None,
            summary: Some("Failed to apply to any branch".to_string()),
        });
    }
    results
}

fn main() {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.decode())
        .unwrap_or_else(|e| e.exit());
    let settings = settings::parse(args.arg_config_file);

    // The HTTP client we'll use to access the APIs
    let client = Arc::new(Client::new());

    let mut patchwork = PatchworkServer::new(&settings.patchwork.url, &client);
    if settings.patchwork.user.is_some() {
        patchwork.set_authentication(&settings.patchwork.user.clone().unwrap(),
                                     &settings.patchwork.pass.clone());
    }
    let patchwork = patchwork;

    if args.flag_mbox != "" && args.flag_project != "" {
        let patch = Path::new(&args.flag_mbox);
        match settings.projects.get(&args.flag_project) {
            None => panic!("Couldn't find project {}", args.flag_project),
            Some(project) => {
                test_patch(&settings, &project, &patch);
            }
        }

        return;
    }

    if args.flag_series > 0 {
        let series = patchwork.get_series(&(args.flag_series as u64)).unwrap();
        match settings.projects.get(&series.project.linkname) {
            None => panic!("Couldn't find project {}", &series.project.linkname),
            Some(project) => {
                let patch = patchwork.get_patch(&series);
                test_patch(&settings, &project, &patch);
            }
        }

        return;
    }

    // The number of series tested so far.  If --count isn't provided, this is unused.
    let mut series_count = 0;

    // Poll patchwork for new series. For each series, get patches, apply and test.
    'daemon: loop {
        let series_list = patchwork.get_series_query().unwrap().results.unwrap();
        for series in series_list {
            // If it's already been tested, we can skip it
            if series.test_state.is_some() {
                continue;
            }

            match settings.projects.get(&series.project.linkname) {
                None => continue,
                Some(project) => {
                    let patch = patchwork.get_patch(&series);
                    let results = test_patch(&settings, &project, &patch);
                    // Delete the temporary directory with the patch in it
                    fs::remove_dir_all(patch.parent().unwrap()).unwrap_or_else(
                        |err| println!("Couldn't delete temp directory: {}", err));
                    if project.push_results {
                        for result in results {
                            patchwork.post_test_result(result, &series.id,
                                                       &series.version).unwrap();
                        }
                    }
                    if args.flag_count > 0 {
                        series_count += 1;
                        if series_count >= args.flag_count {
                            break 'daemon;
                        }
                    }
                }
            }
        }
        thread::sleep(Duration::new(settings.patchwork.polling_interval, 0));
    }
}
