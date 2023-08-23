// snowpatch - continuous integration for patch-based workflows
//
// Copyright (C) 2021 IBM Corporation
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

#![allow(unused_imports)]
#![allow(dead_code)]
#![deny(warnings)]

extern crate clap;
use clap::{arg, command, value_parser};

extern crate ron;

extern crate serde;
extern crate serde_json;

extern crate anyhow;
use anyhow::Result;
use log::{debug, error, info, log_enabled, trace, warn};

extern crate url;

extern crate ureq;
use std::{
    fmt::Display,
    thread,
    time::{Duration, Instant},
    path::PathBuf,
};
use ureq::{Agent, AgentBuilder};

mod patchwork;
use crate::patchwork::PatchworkServer;
mod config;

extern crate log;

mod watchcat;
use crate::watchcat::Watchcat;

extern crate rayon;

#[macro_use]
extern crate lazy_static;

extern crate sled;

extern crate bincode;

use std::ops::Deref;

extern crate git2;

mod git;
use crate::git::GitOps;

mod database;

mod runner;

mod dispatch;
use crate::dispatch::Dispatch;

extern crate dyn_clone;

extern crate dirs;

// This is initialised at runtime and globally accessible.
// Our database is completely thread-safe, so that isn't a problem.
// Let's not have any more global variables than that.
lazy_static! {
    // Yes there's an unwrap, and yes I'd rather parse options and stuff first.
    // Beats trying to pass a db pointer to every corner of the universe...
    pub static ref DB: sled::Db = sled::open("database").unwrap();
}

fn main() -> Result<()> {
    let db_id = DB.generate_id()?;

    let matches = command!()
        .arg(
            arg!(
                -c --config <FILE> "snowpatch requires a config file"
            )
            .required(true)
            .value_parser(value_parser!(PathBuf)),
        )
        .get_matches();

    env_logger::init();

    // unwrap is safe because config is a required value
    let config = matches.get_one::<PathBuf>("config").unwrap();
    let config = config::parse_config(&config)?;

    // XXX let's try and use config as little as possible
    // instead of keeping around the patchwork config,
    // let's use it to make a struct with a URL struct
    // and a token and that's it.

    if DB.was_recovered() {
        dbg!(DB.size_on_disk()?);
    } else {
        dbg!("New database created.");
    }

    dbg!(db_id);

    // create the ureq Agent.  this should only be done once.
    // it can be happily cloned between thread contexts.
    let agent: Agent = AgentBuilder::new()
        .timeout_read(Duration::from_secs(30))
        .timeout_write(Duration::from_secs(30))
        .build();

    let git = GitOps::new(config.git.repo, config.git.workers, config.git.workdir)?;

    rayon::spawn(move || {
        git.init_worktrees().unwrap();
        git.ingest().unwrap();
    });

    let runners: Vec<Box<dyn runner::Runner + Send>> = runner::init(config.runners, agent.clone())?;

    for r in runners {
        rayon::spawn(|| {
            runner::new_job_watcher(r).unwrap();
        });
    }

    // this does a smoke test on creation.
    let patchwork = PatchworkServer::new(
        config.patchwork.url,
        config.patchwork.token,
        agent.clone(),
        config.patchwork.page_size,
    )?;
    let dispatch = Dispatch::new(patchwork.clone());
    rayon::spawn(move || loop {
        dispatch.wait_and_send().unwrap();
    });
    let mut watchcat = Watchcat::new(&config.name, patchwork);
    watchcat.scan()?;

    loop {
        if log_enabled!(log::Level::Trace) {
            for name in DB.tree_names() {
                let tree = DB.open_tree(&name)?;

                trace!(
                    "Tree {} has {} values",
                    String::from_utf8_lossy(&name),
                    tree.iter().count()
                );
            }
        }
        thread::sleep(Duration::from_secs(60));
        if Instant::now()
            .duration_since(watchcat.last_checked)
            .as_secs()
            <= 600
        {
            continue;
        }
        watchcat.last_checked = Instant::now();
        watchcat.scan()?;
    }
}
