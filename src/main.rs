extern crate hyper;
extern crate rustc_serialize;
extern crate git2;

use git2::{Repository, BranchType};
use git2::build::CheckoutBuilder;

use hyper::Client;
use hyper::header::Connection;

use rustc_serialize::json::{self};

use std::io;
use std::fs::File;
use std::str;
use std::path::Path;
use std::process::Command;
use std::string::String;

pub mod api;
use api::Series;

fn main() {
    let api_base = "https://russell.cc/patchwork/api/1.0";
    let project_name = "linuxppc-dev";
    let base_branch = "refs/heads/master";
    let repo_path = "/home/ruscur/Documents/linux/";
    let repo = Repository::open(repo_path).unwrap();
    let client = Client::new();

    // find the latest commit
    let head = repo.head().unwrap();
    let oid = head.target().unwrap();
    let mut commit = repo.find_commit(oid).unwrap();

    println!("Commit: {}", commit.summary().unwrap());

    let url = format!("{}/projects/{}/series/", api_base, project_name);

    let mut resp = client.get(&*url).header(Connection::close()).send().unwrap();
    let mut body: Vec<u8> = vec![];

    println!("Response: {}", resp.status);
    println!("Headers:\n{}", resp.headers);
    io::copy(&mut resp, &mut body).unwrap();

    let body_str = str::from_utf8(&body).unwrap();

    println!("{}", body_str);
    let decoded: Series = json::decode(body_str).unwrap();
    let results = decoded.results.unwrap();
    /*
     * For each series, get patches and apply and test...
     * This will eventually be refactored into a poller.
     * For now, we need to be able to quickly re-test the whole process.
     */
    for i in 0..results.len() {
        let series_id = results[i].id;
        let version = results[i].version;
        let tag = format!("skibot-{}-{}", series_id, version);
        let mbox_path = format!("/tmp/patches/{}.mbox", tag);
        let mbox_url = format!("{}/series/{}/revisions/{}/mbox/", api_base, series_id, version);
        let mut mbox_resp = client.get(&*mbox_url).header(Connection::close()).send().unwrap();
        let path = Path::new(&mbox_path);
        println!("Opening file {}", path.display());
        let mut mbox = File::create(&path).unwrap();
        println!("Writing to file...");
        io::copy(&mut mbox_resp, &mut mbox).unwrap();
        println!("Creating a new branch...");
        let mut branch = repo.branch(&tag, &commit, true).unwrap();
        println!("Switching to branch...");
        repo.set_head(branch.get().name().unwrap());
        repo.checkout_head(None);
        println!("Repo is now at head {}", repo.head().unwrap().name().unwrap());
        let output = Command::new("git").arg("am").arg(&mbox_path).current_dir(repo_path).output().unwrap();
        match output.status.success() {
            true => println!("Patch applied with text {}", String::from_utf8(output.stdout).unwrap()),
            false => {
                Command::new("git").arg("am").arg("--abort").current_dir(repo_path).output().unwrap();
                println!("Patch did not apply successfully");
            },
        }
        repo.set_head(base_branch);
        repo.checkout_head(Some(&mut CheckoutBuilder::new().force()));
        // we need to find the branch again since its head has moved
        branch = repo.find_branch(&tag, BranchType::Local).unwrap();
        branch.delete().unwrap();
        println!("Repo is back to {}", repo.head().unwrap().name().unwrap());
    }
}
