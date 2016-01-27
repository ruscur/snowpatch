// api.rs: structs representing the patchwork API

// /api/1.0/projects/*/series/

#[derive(RustcEncodable,RustcDecodable)]
pub struct Result {
    pub id: u64,
    pub project: u64,
    pub name: String,
    pub n_patches: u64,
    pub submitter: Option<u64>,
    pub submitted: String,
    pub last_updated: String,
    pub version: u64,
    pub reviewer: Option<String>
}

#[derive(RustcEncodable,RustcDecodable)]
pub struct Series {
    pub count: u64,
    pub next: Option<String>,
    pub previous: Option<String>,
    pub results: Option<Vec<Result>>
}
