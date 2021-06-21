/// because cats are better than dogs
///
/// The watchcat exists to monitor state.  It doesn't really care
/// what the rest of snowpatch is doing, it's going to watch Patchwork
/// to see if there's any new stuff to do, and queue up stuff to do.
use anyhow::{Context, Result};
use patchwork::{Check, PatchworkServer, Series, TestState};
use rayon::prelude::*;
use std::convert::TryInto;
use std::ops::Deref;
use std::time::{Duration, Instant};

use DB;

// You should spawn one watchcat per project.
pub struct Watchcat {
    project: String,
    server: PatchworkServer,
    watching: Vec<SeriesState>,
    pub last_checked: Instant,
}

impl Watchcat {
    pub fn new(project: &str, server: PatchworkServer) -> Watchcat {
        Watchcat {
            project: project.to_string(),
            server,
            watching: Vec::new(),
            last_checked: Instant::now(),
        }
    }

    fn check_state(server: &PatchworkServer, series: &Series) -> Result<()> {
        let patch = series.patches.first().context("Series with no patches?")?;
        let checks = server.get_patch_checks(patch.id)?;

        if checks.is_empty() {
            let tree = DB.open_tree(b"needs-testing")?;

            // TODO here the value would be more useful information, probably.
            tree.insert(
                bincode::serialize(&series.id)?,
                bincode::serialize(&series.mbox)?,
            )?;
        }
        /*
                for check in checks {
                    if check.state == TestState::Pending {
                        let prs = DB.get(b"PRs")?;

                        match prs {
                            Some(v) => {
                                let decoded: Check = bincode::deserialize(v.deref())?;
                                println!("WHAT THE FUUUUU {} {}", decoded.user.username, decoded.id);
                            },
                            None =>  {
                                // let's get insane
                                let encoded: Vec<u8> = bincode::serialize(&check)?;
                                DB.insert(b"PRs", encoded);
                            }
                        }
                    }
                }
        */

        Ok(())
    }

    fn check_series_list(&self) -> Result<()> {
        let list = self.server.get_series_list(&self.project)?;

        let results: Vec<Result<()>> = list
            .par_iter()
            .map_with(&self.server, |s, p| Watchcat::check_state(s, p))
            .collect();

        Ok(())
    }

    pub fn scan(&self) -> Result<()> {
        self.check_series_list()
    }
}

struct SeriesState {
    id: u32,
    state: String, // fix obviously
}
