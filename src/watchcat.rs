use anyhow::Result;
use patchwork::{PatchworkServer, Series, TestState};
use rayon::prelude::*;
/// because cats are better than dogs
///
/// The watchcat exists to monitor state.  It doesn't really care
/// what the rest of snowpatch is doing, it's going to watch Patchwork
/// to see if there's any new stuff to do, and queue up stuff to do.
use std::time::{Duration, Instant};

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
        println!("{} {:?}", series.id, server.get_series_state(series.id)?);

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
