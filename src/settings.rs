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
// settings.rs - handle snowpatch settings parsing from TOML
//

use toml;

use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};

use git2::{Error, Repository};

use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::Read;

// TODO: Give more informative error messages when we fail to parse.

#[derive(Deserialize, Clone)]
pub struct Git {
    pub user: String,
    pub public_key: Option<String>,
    pub private_key: String,
    pub passphrase: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct Patchwork {
    pub url: String,
    pub port: Option<u16>,
    // TODO: Enforce (user, pass) XOR token
    pub user: Option<String>,
    pub pass: Option<String>,
    pub token: Option<String>,
    pub polling_interval: u64,
}

// TODO: make this CI server agnostic (i.e buildbot or whatever)
#[derive(Deserialize, Clone)]
pub struct Jenkins {
    pub url: String,
    pub port: Option<u16>,
    // TODO: fail if we only get one of username or token
    pub username: Option<String>,
    pub token: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct Project {
    pub repository: String,
    pub branches: Vec<String>,
    pub test_all_branches: Option<bool>,
    pub remote_name: String,
    pub remote_uri: String,
    pub jobs: Vec<Job>,
    pub push_results: bool,
    pub category: Option<String>,
}

impl Project {
    pub fn get_repo(&self) -> Result<Repository, Error> {
        Repository::open(&self.repository)
    }
}

#[derive(Clone)]
pub struct Job {
    pub job: String,
    pub title: String,
    pub remote: String,
    pub branch: String,
    pub hefty: bool,
    pub warn_on_fail: bool,
    pub parameters: BTreeMap<String, String>,
}

impl<'de> Deserialize<'de> for Job {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct JobVisitor;

        impl<'de> Visitor<'de> for JobVisitor {
            type Value = Job;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct Job with arbitrary fields")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Job, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut job = None;
                let mut title = None;
                let mut remote = None;
                let mut branch = None;
                let mut hefty = None;
                let mut warn_on_fail = None;
                let mut parameters = BTreeMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "job" => {
                            if job.is_some() {
                                return Err(de::Error::duplicate_field("job"));
                            }
                            job = Some(map.next_value()?);
                        }
                        "title" => {
                            if title.is_some() {
                                return Err(de::Error::duplicate_field("title"));
                            }
                            title = Some(map.next_value()?);
                        }
                        "remote" => {
                            if remote.is_some() {
                                return Err(de::Error::duplicate_field("remote"));
                            }
                            remote = Some(map.next_value()?);
                        }
                        "branch" => {
                            if branch.is_some() {
                                return Err(de::Error::duplicate_field("branch"));
                            }
                            branch = Some(map.next_value()?);
                        }
                        "hefty" => {
                            if hefty.is_some() {
                                return Err(de::Error::duplicate_field("hefty"));
                            }
                            hefty = Some(map.next_value()?);
                        }
                        "warn_on_fail" => {
                            if warn_on_fail.is_some() {
                                return Err(de::Error::duplicate_field("warn_on_fail"));
                            }
                            warn_on_fail = Some(map.next_value()?);
                        }
                        _ => {
                            parameters.insert(key, map.next_value()?);
                        }
                    }
                }

                let job: String = job.ok_or_else(|| de::Error::missing_field("job"))?;
                let remote = remote.ok_or_else(|| de::Error::missing_field("remote"))?;
                let branch = branch.ok_or_else(|| de::Error::missing_field("branch"))?;
                let title = title.unwrap_or_else(|| job.clone());
                let hefty = hefty.unwrap_or(false);
                let warn_on_fail = warn_on_fail.unwrap_or(false);

                Ok(Job {
                    job,
                    title,
                    remote,
                    branch,
                    hefty,
                    warn_on_fail,
                    parameters,
                })
            }
        }

        deserializer.deserialize_map(JobVisitor)
    }
}

#[derive(Deserialize, Clone)]
pub struct Config {
    pub git: Git,
    pub patchwork: Patchwork,
    pub jenkins: Jenkins,
    pub projects: BTreeMap<String, Project>,
}

pub fn parse(path: &str) -> Config {
    let mut toml_config = String::new();

    let mut file = File::open(&path).expect("Couldn't open config file, exiting.");

    file.read_to_string(&mut toml_config)
        .unwrap_or_else(|err| panic!("Couldn't read config: {}", err));

    let toml_config = toml::de::from_str::<Config>(&toml_config);

    match toml_config {
        Ok(config_inside) => config_inside,
        Err(err) => {
            error!("TOML error: {}", err);
            panic!("Could not parse configuration file, exiting");
        }
    }
}

#[cfg(test)]
mod test {
    use settings::*;

    #[test]
    #[should_panic(expected = "Couldn't open config file")]
    fn bad_path() {
        parse("/nonexistent/config.file");
    }

    #[test]
    fn parse_example_openpower() {
        parse("examples/openpower.toml");
    }

    #[test]
    #[should_panic(expected = "Could not parse configuration file, exiting")]
    fn parse_example_invalid() {
        parse("examples/tests/invalid.toml");
    }
}
