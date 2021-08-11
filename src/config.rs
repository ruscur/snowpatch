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
// config.rs - handle snowpatch config parsing from RON
//

// standard library
use std::{env, fs::File, path::PathBuf};

// third party dependencies
use anyhow::{Context, Result};
use dirs::home_dir;
use ron::de::from_reader;
use serde::Deserialize;
use url::Url;

// snowpatch stuff
use crate::DB;

/// Defines the full set of information snowpatch needs in order to do anything useful.
#[derive(Debug, Deserialize)]
pub struct Config {
    name: String,
    pub git: Git,
    pub patchwork: Patchwork,
    pub runners: Vec<Runner>,
}

/// Defines the git details snowpatch needs to push to remotes.
/// snowpatch uses libgit2 in order to communicate with remotes through SSH.
#[derive(Debug, Deserialize)]
pub struct Git {
    /// The user on the remote, typically `git` for most services, as in `git@github.com:ruscur/snowpatch.git`
    user: String,
    /// Full path to the public key, for example `/home/ruscur/.ssh/id_rsa.pub`
    /// Defaults to `~/.ssh/id_rsa.pub`
    #[serde(default = "default_public_key")]
    public_key: PathBuf,
    /// Full path to the private key, for example `/home/ruscur/.ssh/id_rsa`
    /// Defaults to `~/.ssh/id_rsa`
    #[serde(default = "default_private_key")]
    private_key: PathBuf,
    /// Full path of the git repo to work with
    pub repo: String,
    /// Path that you want snowpatch to make git worktrees in
    pub workdir: String,
    /// Max number of worktrees created at once
    #[serde(default = "default_workers")]
    pub workers: usize,
}

fn default_public_key() -> PathBuf {
    let mut path: PathBuf = dirs::home_dir().unwrap_or(PathBuf::from("/"));

    path.push(".ssh");
    path.push("id_rsa.pub");

    path
}

fn default_private_key() -> PathBuf {
    let mut path: PathBuf = dirs::home_dir().unwrap_or(PathBuf::from("/"));

    path.push(".ssh");
    path.push("id_rsa");

    path
}

fn default_workers() -> usize {
    1
}

/// Defines the Patchwork server you wish to work with.
/// Credentials are not necessary unless you wish to push results.
/// snowpatch only supports API token authentication and not Basic Auth.
#[derive(Debug, Deserialize)]
pub struct Patchwork {
    /// URL of the Patchwork server, without port i.e. `https://patchwork.ozlabs.org`
    pub url: Url,
    /// API token you wish to use on the Patchwork server, only needed if pushing results
    pub token: Option<String>,
    /// Number of series to request at a time.  Defaults to 50.
    #[serde(default = "default_page_size")]
    pub page_size: u64,
}

fn default_page_size() -> u64 {
    50
}

// Runners
#[derive(Debug, Deserialize)]
pub enum Runner {
    GitHub {
        trigger: Trigger,
        url: Url,
        token: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
pub enum Trigger {
    OnPush { remote: String },
    Manual { data: String },
}

fn validate_config(config: &Config) -> Result<()> {
    // Validate paths
    File::open(&config.git.public_key).context("Couldn't open public key file")?;
    File::open(&config.git.private_key).context("Couldn't open private key file")?;

    Ok(())
}

/// Puts some static values in the database.
fn populate_database(config: &Config) -> Result<()> {
    DB.insert(
        "ssh private key path",
        config
            .git
            .private_key
            .to_str()
            .context("Something went wrong with SSH private key path")?,
    )?;
    DB.insert(
        "ssh public key path",
        config
            .git
            .public_key
            .to_str()
            .context("Something went wrong with SSH public key path")?,
    )?;
    DB.insert(
        "patchwork series link prefix",
        format!(
            "{}/project/{}/list/?series=",
            config.patchwork.url, config.name
        )
        .as_bytes(),
    )?;

    Ok(())
}

pub fn parse_config(filename: &str) -> Result<Config> {
    let file = File::open(filename)
        .with_context(|| format!("Failed to open config file at {}", filename))?;

    let config: Config =
        from_reader(file).with_context(|| format!("Failed to parse config file"))?;

    dbg!("Config: {:?}", &config);

    validate_config(&config)?;

    populate_database(&config)?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_good_config() {
        let good_config: Config = Config {
            name: "linuxppc-dev".to_owned(),
            git: Git {
                user: "git".to_owned(),
                public_key: default_public_key(),
                private_key: default_private_key(),
                repo: "/home/ruscur/code/linux".to_owned(),
                workdir: "/home/ruscur/code/snowpatch/workdir".to_owned(),
                workers: 2,
            },
            patchwork: Patchwork {
                url: Url::parse("https://patchwork.ozlabs.org").unwrap(),
                token: None,
                page_size: default_page_size(),
            },
            // TODO
            runners: vec![],
        };

        println!("{:?}", good_config);

        assert!(validate_config(&good_config).is_ok());
    }

    #[test]
    fn parse_good_config() {
        assert!(parse_config("examples/tests/valid.ron").is_ok());
    }
}
