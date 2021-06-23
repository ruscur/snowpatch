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
use std::fs::File;

// third party dependencies
use anyhow::{Context, Result};
use ron::de::from_reader;
use serde::Deserialize;
use url::Url;

/// Defines the full set of information snowpatch needs in order to do anything useful.
#[derive(Debug, Deserialize)]
pub struct Config {
    name: String,
    pub git: Git,
    pub patchwork: Patchwork,
}

/// Defines the git details snowpatch needs to push to remotes.
/// snowpatch uses libgit2 in order to communicate with remotes through SSH.
#[derive(Debug, Deserialize)]
pub struct Git {
    /// The user on the remote, typically `git` for most services, as in `git@github.com:ruscur/snowpatch.git`
    user: String,
    /// Full path to the public key, for example `/home/ruscur/.ssh/id_rsa.pub`
    public_key: String,
    /// Full path to the private key, for example `/home/ruscur/.ssh/id_rsa`
    private_key: String,
    /// Full path of the git repo to work with
    pub repo: String,
    /// Path that you want snowpatch to make git worktrees in
    pub workdir: String,
    /// Max number of worktrees created at once
    #[serde(default = "default_workers")]
    pub workers: usize,
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
    pub url: String,
    /// API token you wish to use on the Patchwork server, only needed if pushing results
    pub token: Option<String>,
}

fn validate_config(config: &Config) -> Result<()> {
    // Validate paths
    File::open(&config.git.public_key).with_context(|| format!("Couldn't open public key file"))?;
    File::open(&config.git.private_key)
        .with_context(|| format!("Couldn't open private key file"))?;

    // Validate URLs
    Url::parse(&config.patchwork.url).with_context(|| format!("Couldn't parse Patchwork URL"))?;

    Ok(())
}

pub fn parse_config(filename: &str) -> Result<Config> {
    let file = File::open(filename)
        .with_context(|| format!("Failed to open config file at {}", filename))?;

    let config: Config =
        from_reader(file).with_context(|| format!("Failed to parse config file"))?;

    dbg!("Config: {:?}", &config);

    validate_config(&config)?;

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
                public_key: "/home/ruscur/.ssh/id_rsa.pub".to_owned(),
                private_key: "/home/ruscur/.ssh/id_rsa".to_owned(),
            },
            patchwork: Patchwork {
                url: "https://patchwork.ozlabs.org".to_owned(),
                token: None,
            },
        };

        assert!(validate_config(&good_config).is_ok());
    }

    #[test]
    fn parse_good_config() {
        assert!(parse_config("examples/tests/valid.ron").is_ok());
    }
}
