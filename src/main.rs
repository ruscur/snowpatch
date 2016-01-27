extern crate hyper;
extern crate rustc_serialize;

use hyper::Client;
use hyper::header::Connection;

use rustc_serialize::json::{self};

use std::io;
use std::fs::File;
use std::str;
use std::path::Path;

pub mod api;
use api::Series;

fn main() {
    let api_base = "https://russell.cc/patchwork/api/1.0";
    let project_name = "linuxppc-dev";
    let client = Client::new();

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
        let mbox_path = format!("/tmp/patches/{}-{}-{}.mbox", series_id, version,
                                results[i].name.replace("/", "-"));
        let mbox_url = format!("{}/series/{}/revisions/{}/mbox/", api_base, series_id, version);
        let mut mbox_resp = client.get(&*mbox_url).header(Connection::close()).send().unwrap();
        let path = Path::new(&mbox_path);
        println!("Opening file {}", path.display());
        let mut mbox = File::create(&path).unwrap();
        println!("Writing to file...");
        io::copy(&mut mbox_resp, &mut mbox).unwrap();
    }
}
