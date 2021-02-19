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

#![deny(warnings)]

extern crate clap;
use clap::{App, Arg};

extern crate ron;

extern crate serde;

extern crate anyhow;
use anyhow::Result;

extern crate url;

mod config;

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

    // unwrap is safe because config is a required value
    let config = matches.value_of("config").unwrap();
    config::parse_config(config)?;

    Ok(())
}
