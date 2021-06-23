use crate::patchwork::{Check, PatchworkServer, Series, TestState};
/// because cats are better than dogs
///
/// The watchcat exists to monitor state.  It doesn't really care
/// what the rest of snowpatch is doing, it's going to watch Patchwork
/// to see if there's any new stuff to do, and queue up stuff to do.
///
/// The watchcat does not test anything.
/// It just queues things to be tested, checks in to see if any paper needs pushing,
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::time::{Duration, Instant};

use crate::DB;

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
            let tree = DB.open_tree(b"needs testing")?;

            // TODO here the value would be more useful information, probably.
            println!("Inserting {}!", series.id);
            tree.insert(
                bincode::serialize(&series.id)?,
                bincode::serialize(&series.mbox)?,
            )?;
        }

        Ok(())
    }

    fn check_series_list(&self) -> Result<()> {
        let list = self.server.get_series_list(&self.project)?;

        let results: Result<Vec<()>> = list
            .par_iter()
            .map_with(&self.server, |s, p| Watchcat::check_state(s, p))
            .collect();

        let _ = results?;

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
