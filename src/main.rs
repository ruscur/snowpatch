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

// I SOLEMNLY SWEAR TO UNCOMMENT THIS
//#![deny(warnings)]

extern crate clap;
use clap::{App, Arg};

extern crate ron;

extern crate serde;
extern crate serde_json;

extern crate anyhow;
use anyhow::Result;

extern crate url;

extern crate ureq;
use std::{
    thread,
    time::{Duration, Instant},
};
use ureq::{Agent, AgentBuilder};

mod patchwork;
use patchwork::PatchworkServer;
mod config;

extern crate log;

mod watchcat;
use watchcat::Watchcat;

extern crate rayon;

fn main() -> Result<()> {
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

    // create the ureq Agent.  this should only be done once.
    // it can be happily cloned between thread contexts.
    let agent: Agent = AgentBuilder::new()
        .timeout_read(Duration::from_secs(10))
        .timeout_write(Duration::from_secs(10))
        .build();

    // this does a smoke test on creation.
    let patchwork =
        PatchworkServer::new(config.patchwork.url, config.patchwork.token, agent.clone())?;
    let mut watchcat = Watchcat::new("linuxppc-dev", patchwork);
    watchcat.scan()?;

    loop {
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

    Ok(())
}
