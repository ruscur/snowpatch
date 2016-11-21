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
extern crate url;
#[macro_use]
extern crate log;
extern crate env_logger;

use git2::{Cred, BranchType, RemoteCallbacks, PushOptions};

use hyper::Client;

use docopt::Docopt;

use url::Url;

use log::LogLevelFilter;
use env_logger::LogBuilder;

use std::fs;
use std::string::String;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::path::Path;
use std::env;

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
  --count <count>       Run tests on the <count> most recent patches.
  --patch <id>          Run tests on the given Patchwork patch.
  --series <id>         Run tests on the given Patchwork series.
  --mbox <mbox>         Run tests on the given mbox file...
  --project <name>      ...as if it were sent to project <name>.
  -v, --version         Output version information.
  -h, --help            Output this help text.
";

#[derive(RustcDecodable)]
struct Args {
    arg_config_file: String,
    flag_count: u16,
    flag_patch: u32,
    flag_series: u32,
    flag_mbox: String,
    flag_project: String,
}

fn run_tests(settings: &Config, client: Arc<Client>, project: &Project, tag: &str,
             branch_name: &str, hefty_tests: bool) -> Vec<TestResult> {
    let mut results: Vec<TestResult> = Vec::new();
    let jenkins = JenkinsBackend {
        base_url: settings.jenkins.url.clone(),
        hyper_client: client,
    };
    let project = project.clone();
    for job_params in project.jobs.iter() {
        let job_name = job_params.get("job").unwrap();
        if !hefty_tests && settings::job_is_hefty(job_params) {
            debug!("Skipping hefty test {}", job_name);
            continue;
        }
        let mut jenkins_params = Vec::<(&str, &str)>::new();
        for (param_name, param_value) in job_params.iter() {
            debug!("Param name {}, value {}", &param_name, &param_value);
            match param_name.as_ref() {
                // TODO: Validate special parameter names in config at start of program
                "job" => { },
                "remote" => jenkins_params.push((&param_value, &project.remote_uri)),
                "branch" => jenkins_params.push((&param_value, &tag)),
                _ => jenkins_params.push((&param_name, &param_value)),
            }
        }
        info!("Starting job: {}", &job_name);
        let res = jenkins.start_test(&job_name, jenkins_params)
            .unwrap_or_else(|err| panic!("Starting Jenkins test failed: {}", err));
        debug!("{:?}", &res);
        let build_url_real;
        loop {
            let build_url = jenkins.get_build_url(&res);
            match build_url {
                Some(url) => { build_url_real = url; break; },
                None => { },
            }
        }
        debug!("Build URL: {}", build_url_real);
        jenkins.wait_build(&build_url_real);
        info!("Jenkins job for {}/{} complete.", branch_name, job_name);
        // TODO: actually get results from jenkins
        results.push(TestResult {
            state: TestState::SUCCESS.string(),
            description: Some(format!("{}/{}", branch_name.to_string(),
                                      job_name.to_string()).to_string()),
            .. Default::default()
        });
    }
    results
}

fn test_patch(settings: &Config, client: &Arc<Client>, project: &Project,
              path: &Path, hefty_tests: bool) -> Vec<TestResult> {
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
        info!("Configuring local branch for {}.", tag);
        debug!("Switching to base branch {}...", branch_name);
        git::checkout_branch(&repo, &branch_name);

        // Make sure we're up to date
        git::pull(&repo).unwrap();

        debug!("Creating a new branch...");
        let mut branch = repo.branch(&tag, &commit, true).unwrap();
        debug!("Switching to branch...");
        repo.set_head(branch.get().name().unwrap())
            .unwrap_or_else(|err| panic!("Couldn't set HEAD: {}", err));
        repo.checkout_head(None)
            .unwrap_or_else(|err| panic!("Couldn't checkout HEAD: {}", err));
        debug!("Repo is now at head {}", repo.head().unwrap().name().unwrap());

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
        debug!("Repo is back to {}", repo.head().unwrap().name().unwrap());

        match output {
            Ok(_) => {
                successfully_applied = true;
                results.push(TestResult {
                    state: TestState::SUCCESS.string(),
                    description: Some(format!("{}/{}\n\n{}",
                                              branch_name.to_string(),
                                              "apply_patch".to_string(),
                                              "Successfully applied".to_string())
                                      .to_string()),
                    .. Default::default()
                });
            },
            Err(_) => {
                // It didn't apply.  No need to bother testing.
                results.push(TestResult {
                    state: TestState::WARNING.string(),
                    description: Some(format!("{}/{}\n\n{}",
                                              branch_name.to_string(),
                                              "apply_patch".to_string(),
                                              "Patch failed to apply".to_string())
                                      .to_string()),
                    .. Default::default()
                });
                continue;
            }
        }

        let settings = settings.clone();
        let project = project.clone();
        let client = client.clone();
        let settings_clone = settings.clone();
        let test_all_branches = project.test_all_branches.unwrap_or(true);

        // We've set up a remote branch, time to kick off tests
        let test = thread::Builder::new().name(tag.to_string()).spawn(move || {
            return run_tests(&settings_clone, client, &project, &tag,
                             &branch_name, hefty_tests);
        }).unwrap();
        results.append(&mut test.join().unwrap());

        if !test_all_branches { break; }
    }

    if !successfully_applied {
        results.push(TestResult {
            state: TestState::FAILURE.string(),
            description: Some("Failed to apply to any branch".to_string()),
            .. Default::default()
        });
    }
    results
}

fn main() {
    let mut log_builder = LogBuilder::new();
    // By default, log at the "info" level for every module
    log_builder.filter(None, LogLevelFilter::Info);
    if env::var("RUST_LOG").is_ok() {
        log_builder.parse(&env::var("RUST_LOG").unwrap());
    }
    log_builder.init().unwrap();

    let version = format!("{} version {}",
        env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.version(Some(version)).decode())
        .unwrap_or_else(|e| e.exit());

    let settings = settings::parse(args.arg_config_file);

    // The HTTP client we'll use to access the APIs
    // TODO: HTTPS support, not yet implemented in Hyper as of 0.9.6
    let client = Arc::new(match env::var("http_proxy") {
        Ok(proxy_url) => {
            debug!("snowpatch is using HTTP proxy {}", proxy_url);
            let proxy = Url::parse(&proxy_url).unwrap_or_else(|e| {
                panic!("http_proxy is malformed: {:?}, error: {}", proxy_url, e);
            });
            assert!(proxy.has_host());
            assert!(proxy.scheme() == "http");
            // This should pass even if no trailing slash is in http_proxy
            assert!(proxy.path() == "/");
            Client::with_http_proxy(proxy.host_str().unwrap().to_string(),
                proxy.port().unwrap_or(80))
        },
        _ => {
            debug!("snowpatch starting without a HTTP proxy");
            Client::new()
        }
    });

    let mut patchwork = PatchworkServer::new(&settings.patchwork.url, &client);
    if settings.patchwork.user.is_some() {
        debug!("Patchwork authentication set for user {}",
               &settings.patchwork.user.clone().unwrap());
        patchwork.set_authentication(&settings.patchwork.user.clone().unwrap(),
                                     &settings.patchwork.pass.clone());
    }
    let patchwork = patchwork;

    if args.flag_series > 0 && args.flag_patch > 0 {
        panic!("Can't specify both --series and --patch");
    }

    if args.flag_mbox != "" && args.flag_project != "" {
        info!("snowpatch is testing a local patch.");
        let patch = Path::new(&args.flag_mbox);
        match settings.projects.get(&args.flag_project) {
            None => panic!("Couldn't find project {}", args.flag_project),
            Some(project) => {
                test_patch(&settings, &client, &project, &patch, true);
            }
        }

        return;
    }

    if args.flag_patch > 0 {
        info!("snowpatch is testing a patch from Patchwork.");
        let patch = patchwork.get_patch(&(args.flag_patch as u64)).unwrap();
        let project = patchwork.get_project(&patch.project).unwrap();
        match settings.projects.get(&project.link_name) {
            None => panic!("Couldn't find project {}", &project.link_name),
            Some(project) => {
                let mbox;

                if patch.has_series() {
                    let dependencies = patchwork.get_patch_dependencies(&patch);
                    mbox = patchwork.get_patches_mbox(dependencies);
                } else {
                    mbox = patchwork.get_patch_mbox(&patch);
                }
                test_patch(&settings, &client, &project, &mbox, true);
            }
        }
        return;
    }

    if args.flag_series > 0 {
        info!("snowpatch is testing a series from Patchwork.");
        let series = patchwork.get_series(&(args.flag_series as u64)).unwrap();
        // The last patch in the series, so its dependencies are the whole series
        let patch = patchwork.get_patch_by_url(series.patches.last().unwrap()).unwrap();
        // We have to do it this way since there's no project field on Series
        let project = patchwork.get_project(&patch.project).unwrap();
        match settings.projects.get(&project.link_name) {
            None => panic!("Couldn't find project {}", &project.link_name),
            Some(project) => {
                let dependencies = patchwork.get_patch_dependencies(&patch);
                let mbox = patchwork.get_patches_mbox(dependencies);
                test_patch(&settings, &client, &project, &mbox, true);
            }
        }
        return;
    }

    // The number of patches tested so far.  If --count isn't provided, this is unused.
    let mut patch_count = 0;

    /*
     * Poll Patchwork for new patches.
     * If the patch is standalone (not part of a series), apply it.
     * If the patch is part of a series, apply all of its dependencies.
     * Spawn tests.
     */
    'daemon: loop {
        let patch_list = patchwork.get_patch_query().unwrap();
        info!("snowpatch is ready to test new revisions from Patchwork.");
        for patch in patch_list {
            // If it's already been tested, we can skip it
            if patch.check != "pending" {
                debug!("Skipping already tested patch {}", patch.name);
                continue;
            }

            let project = patchwork.get_project(&patch.project).unwrap();

            match settings.projects.get(&project.link_name) {
                None => {
                    debug!("Project {} not configured for patch {}",
                           &project.link_name, patch.name);
                    continue;
                },
                Some(project) => {
                    let mbox;
                    let hefty_tests;

                    if patch.has_series() {
                        debug!("Patch {} has a series at {}!", &patch.name, &patch.series[0]);
                        let dependencies = patchwork.get_patch_dependencies(&patch);
                        let series = patchwork.get_series_by_url(&patch.series[0]).unwrap();
                        hefty_tests = dependencies.len() == series.patches.len();
                        mbox = patchwork.get_patches_mbox(dependencies);
                    } else {
                        hefty_tests = true;
                        mbox = patchwork.get_patch_mbox(&patch);
                    }

                    let results = test_patch(&settings, &client, &project, &mbox, hefty_tests);
                    // Delete the temporary directory with the patch in it
                    fs::remove_dir_all(mbox.parent().unwrap()).unwrap_or_else(
                        |err| error!("Couldn't delete temp directory: {}", err));
                    if project.push_results {
                        for result in results {
                            patchwork.post_test_result(result, &patch.checks).unwrap();
                        }
                    }
                    if args.flag_count > 0 {
                        patch_count += 1;
                        debug!("Tested {} patches out of {}",
                               patch_count, args.flag_count);
                        if patch_count >= args.flag_count {
                            break 'daemon;
                        }
                    }
                }
            }
        }
        info!("Finished testing new revisions, sleeping.");
        thread::sleep(Duration::new(settings.patchwork.polling_interval, 0));
    }
}
