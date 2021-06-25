//
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
use clap::{App, Arg};

extern crate ron;

extern crate serde;
extern crate serde_json;

extern crate anyhow;
use anyhow::Result;
use log::{debug, error, info, log_enabled, warn};

extern crate url;

extern crate ureq;
use std::{
    fmt::Display,
    thread,
    time::{Duration, Instant},
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

    let matches = App::new("snowpatch")
        .version("1.0")
        .author("Russell Currey <ruscur@russell.cc>")
        .about("Connects to Patchwork to automate CI workflows")
        .arg(
            Arg::with_name("config")
                .value_name("FILE")
                .help("snowpatch requires a config file, see the docs for details")
                .takes_value(true)
                .required(true),
        )
        .get_matches();

    env_logger::init();

    // unwrap is safe because config is a required value
    let config = matches.value_of("config").unwrap();
    let config = config::parse_config(config)?;

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
        .timeout_read(Duration::from_secs(10))
        .timeout_write(Duration::from_secs(10))
        .build();

    let git = GitOps::new(config.git.repo, config.git.workers, config.git.workdir)?;

    rayon::spawn(move || {
        git.init_worktrees().unwrap();
        git.ingest().unwrap();
    });

    // this does a smoke test on creation.
    let patchwork =
        PatchworkServer::new(config.patchwork.url, config.patchwork.token, agent.clone())?;
    let mut watchcat = Watchcat::new("linuxppc-dev", patchwork);
    watchcat.scan()?;

    loop {
        if log_enabled!(log::Level::Debug) {
            for name in DB.tree_names() {
                let tree = DB.open_tree(&name)?;

                debug!(
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
