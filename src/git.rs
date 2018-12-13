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
// git.rs - snowpatch git functionality
//

use git2::build::CheckoutBuilder;
use git2::{Branch, Commit, Cred, Error, PushOptions, Remote, Repository};

use std::io::ErrorKind;
use std::path::Path;
use std::process::{Command, Output};
use std::result::Result;

use settings::Git;

pub static GIT_REF_BASE: &'static str = "refs/heads";

pub fn get_latest_commit(repo: &Repository) -> Result<Commit, Error> {
    let head = repo.head()?;
    let oid = head.target();
    match oid {
        Some(oid) => repo.find_commit(oid),
        None => Err(Error::from_str("Couldn't find latest commit in repo")),
    }
}

pub fn push_to_remote(
    remote: &mut Remote,
    branch: &Branch,
    delete: bool,
    mut opts: &mut PushOptions,
) -> Result<(), Error> {
    let action = if delete { ":" } else { "+" };
    let name = branch.name()?;
    // The unwrap on the name is there because it returns Ok(None) if the branch
    // name isn't valid UTF8.  That's not going to happen for us, and if it does,
    // we deserve to explode.
    let refspecs: &[&str] = &[&format!("{}{}/{}", action, GIT_REF_BASE, name.unwrap())];
    remote.push(refspecs, Some(&mut opts))
}

// TODO: rewrite this to use git2-rs, I think it's impossible currently
pub fn pull(repo: &Repository) -> Result<Output, std::io::Error> {
    let workdir = match repo.workdir() {
        // TODO: support bare repositories
        Some(dir) => dir,
        None => {
            return Err(std::io::Error::new(
                ErrorKind::NotFound,
                "Couldn't find repo workdir",
            ))
        }
    };

    let output = Command::new("git")
        .arg("pull") // pull the cool kid's way
        .current_dir(&workdir) // in the repo's working directory
        .output()?; // run synchronously

    debug!(
        "Pull: {}",
        String::from_utf8(output.clone().stdout).unwrap_or("Invalid UTF8".to_string())
    );
    Ok(output)
}

pub fn checkout_branch(repo: &Repository, branch: &str) -> Result<(), Box<std::error::Error>> {
    let workdir = match repo.workdir() {
        // TODO: support bare repositories
        Some(dir) => Ok(dir),
        None => Err(std::io::Error::new(
            ErrorKind::NotFound,
            "Couldn't find repo workdir",
        )),
    }?;

    // Make sure there's no junk lying around before we switch
    Command::new("git")
        .arg("reset")
        .arg("--hard")
        .current_dir(&workdir)
        .output()?;

    Command::new("git")
        .arg("clean")
        .arg("-f") // force remove files we don't need
        .arg("-d") // ...including directories
        .current_dir(&workdir)
        .output()?;

    repo.set_head(&format!("{}/{}", GIT_REF_BASE, &branch))?;
    repo.checkout_head(Some(&mut CheckoutBuilder::new().force()))?;

    // Clean up again, just to be super sure
    Command::new("git")
        .arg("reset")
        .arg("--hard")
        .current_dir(&workdir)
        .output()?;

    Command::new("git")
        .arg("clean")
        .arg("-f") // force remove files we don't need
        .arg("-d") // ...including directories
        .current_dir(&workdir)
        .output()?;
    Ok(())
}

pub fn apply_patch(repo: &Repository, path: &Path) -> Result<Output, std::io::Error> {
    let workdir = match repo.workdir() {
        // TODO: support bare repositories
        Some(dir) => dir,
        None => {
            return Err(std::io::Error::new(
                ErrorKind::NotFound,
                "Couldn't find repo workdir",
            ))
        }
    };

    // We call out to "git am" since libgit2 doesn't implement "am"
    let output = Command::new("git")
        .arg("am") // apply from mbox
        .arg("-3") // three way merge
        .arg(&path) // from our mbox file
        .current_dir(&workdir) // in the repo's working directory
        .output()?; // run synchronously

    if output.status.success() {
        debug!(
            "Patch applied with text {}",
            String::from_utf8(output.clone().stdout).unwrap_or("Invalid UTF8".to_string())
        );
        Ok(output)
    } else {
        info!(
            "Patch failed to apply with text {} {}",
            String::from_utf8(output.clone().stdout).unwrap_or("Invalid UTF8".to_string()),
            String::from_utf8(output.clone().stderr).unwrap_or("Invalid UTF8".to_string())
        );
        Command::new("git")
            .arg("am")
            .arg("--abort")
            .current_dir(&workdir)
            .output()
    }
}

pub fn cred_from_settings(settings: &Git) -> Result<Cred, Error> {
    // We have to convert from Option<String> to Option<&str>
    let public_key = settings.public_key.as_ref().map(String::as_ref);
    let passphrase = settings.passphrase.as_ref().map(String::as_ref);

    Cred::ssh_key(
        &settings.user,
        public_key,
        Path::new(&settings.private_key),
        passphrase,
    )
}
