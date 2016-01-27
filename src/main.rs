extern crate hyper;
extern crate rustc_serialize;

use hyper::Client;
use hyper::header::Connection;

use rustc_serialize::json::{self};

use std::io;
use std::str;

pub mod api;
use api::Series;

fn main() {
    let url = "https://russell.cc/patchwork/api/1.0/projects/linuxppc-dev/series/";
    let client = Client::new();

    let mut resp = client.get(&*url).header(Connection::close()).send().unwrap();
    let mut body: Vec<u8> = vec![];

    println!("Response: {}", resp.status);
    println!("Headers:\n{}", resp.headers);
    io::copy(&mut resp, &mut body).unwrap();

    let body_str = str::from_utf8(&body).unwrap();

    println!("{}", body_str);
    let decoded: Series = json::decode(body_str).unwrap();
    let results = decoded.results.unwrap();
    for i in 0..results.len() {
        println!("{}", results[i].name);
    }
}
