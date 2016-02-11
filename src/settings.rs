/* TODO: copyright header */
use toml;
use toml::{Parser, Value};
use std::fs::File;
use std::io::Read;
use std::collections::BTreeMap;

// settings.rs: code to handle snowpatch settings parsing from TOML
#[derive(RustcDecodable)]
struct Patchwork {
    url: String,
    port: Option<u16>,
    user: Option<String>,
    pass: Option<String>
}

// TODO: make this CI server agnostic (i.e buildbot or whatever)
#[derive(RustcDecodable)]
struct Jenkins {
    url: String,
    port: Option<u16>,
    token: Option<String>
}

#[derive(RustcDecodable)]
struct Project {
    repository: String,
    branch: String,
    remote: String,
    jobs: Vec<String>,
    push_results: Option<bool>
}

#[derive(RustcDecodable)]
pub struct Config {
    patchwork: Patchwork,
    jenkins: Jenkins,
    projects: BTreeMap<String, Project>
}

pub fn parse(path: String) -> Config {
    let mut toml_config = String::new();

    let mut file = match File::open(&path) {
        Ok(file) => file,
        Err(_) => panic!("Couldn't open config file, exiting.")
    };

    file.read_to_string(&mut toml_config)
        .unwrap_or_else(|err| panic!("Couldn't read config: {}", err));

    let mut parser = Parser::new(&toml_config);
    let toml = parser.parse();

    if toml.is_none() {
        panic!("Syntax error in TOML file, exiting.");
    }

    let config = Value::Table(toml.unwrap());

    match toml::decode(config) {
        Some(t) => t,
        None => panic!("Couldn't deserialize config, exiting.")
    }
}
