/// Does stuff after something has finished testing
use anyhow::{Context, Result};
use log::{error, info};

use crate::database::wait_for_tree;
use crate::patchwork::{PatchworkServer, TestResult, TestState};
use crate::runner::RunnerResult;
use crate::DB;

pub struct Dispatch {
    server: PatchworkServer,
}

impl Dispatch {
    pub fn new(server: PatchworkServer) -> Dispatch {
        Dispatch { server }
    }

    pub fn wait_and_send(&self) -> Result<()> {
        let tree = DB.open_tree("needs dispatch")?;

        loop {
            let mut keys_to_drop = vec![];
            for result in tree.iter() {
                let (db_key, value) = result?;
                let key = String::from_utf8_lossy(&db_key);
                info!("Sending result for {} to Patchwork", key);
                let mut parts = key.split(" ");
                let handle = parts.next().context(format!("Malformed entry: {}", key))?;
                let series = parts.next().context(format!("Malformed entry: {}", key))?;
                let job_name: Vec<String> = parts.map(|s| s.to_string()).collect();
                let job_name = job_name.join(" ");
                let job_result: RunnerResult = bincode::deserialize(&value)?;

                let context: String = format!("{}-{}", handle, job_name);

                let check_to_send = TestResult {
                    state: job_result.outcome,
                    target_url: match job_result.url {
                        Some(url) => Some(url.to_string()),
                        None => None,
                    },
                    description: job_result.description,
                    context: Some(context),
                };

                self.server
                    .send_check(series.parse::<u64>()?, &check_to_send)?;
                keys_to_drop.push(db_key);
            }

            for key in keys_to_drop {
                tree.remove(&key)?;
            }
            wait_for_tree(&tree);
        }
    }
}
