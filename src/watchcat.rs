/// because cats are better than dogs
///
/// The watchcat exists to monitor state.  It doesn't really care
/// what the rest of snowpatch is doing, it's going to watch Patchwork
/// to see if there's any new stuff to do, and queue up stuff to do.
///
/// The watchcat does not test anything.
/// It just queues things to be tested, checks in to see if any paper needs pushing,
use crate::patchwork::{Check, PatchworkServer, Series, TestState};
use anyhow::{Context, Result};
use log::{debug, log_enabled};
use rayon::prelude::*;
use std::time::{Duration, Instant};
use url::Url;

use crate::DB;

// You should spawn one watchcat per project.
pub struct Watchcat {
    project: String,
    server: PatchworkServer,
    pub last_checked: Instant,
}

impl Watchcat {
    pub fn new(project: &str, server: PatchworkServer) -> Watchcat {
        Watchcat {
            project: project.to_string(),
            server,
            last_checked: Instant::now(),
        }
    }

    fn check_state(server: &PatchworkServer, series: &Series) -> Result<()> {
        let patch = series
            .patches
            .first()
            .context(format!("Series with no patches? {}", series.id))?;
        let checks = server.get_patch_checks(patch.id)?;

        // TODO need consolidation between this and the filters
        if checks.is_empty() || true {
            let tree = DB.open_tree(b"needs testing")?;

            // TODO here the value would be more useful information, probably.
            debug!("Inserting {} into git queue", series.id);
            tree.insert(
                bincode::serialize(&series.id)?,
                bincode::serialize(&series.mbox)?,
            )?;
        }

        Ok(())
    }

    fn check_series_list(&self) -> Result<()> {
        let list = self.server.get_series_list(&self.project)?;
        let tree = DB.open_tree("seen by watchcat")?;

        let results: Result<Vec<()>> = list
            .par_iter()
            .filter(|series| series.received_all)
            .filter(|series| series.received_total > 0)
            .filter(|series| !tree.contains_key(&series.id.to_string()).unwrap_or(false))
            .filter(|series| -> bool {
                /*                if log_enabled!(log::Level::Debug) {
                    return true;
                }; */
                if let Some(last_patch) = series.patches.last() {
                    if let Ok(patch) = self.server.get_patch(last_patch.id) {
                        patch.action_required()
                    } else {
                        false
                    }
                } else {
                    false
                }
            })
            .map_with(&self.server, |server, series| {
                tree.insert(&series.id.to_string(), b"hello")?;
                Watchcat::check_state(server, series)
            })
            .collect();

        let _ = results?;

        Ok(())
    }

    pub fn scan(&self) -> Result<()> {
        debug!("Scanning patchwork for new series...");
        self.check_series_list()
    }
}
