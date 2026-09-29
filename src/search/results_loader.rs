//! Background thread that runs searches on the site and returns their results; draws nothing.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::sources::anime_sama::{self, Link};

/// A query and its results.
pub struct Found {
    pub query: String,
    pub result: Result<Vec<Link>, String>,
}

pub struct ResultsLoader {
    queries: Sender<String>,
    pub found: Receiver<Found>,
}

impl ResultsLoader {
    pub fn spawn() -> Self {
        let (queries, query_rx) = mpsc::channel();
        let (found_tx, found) = mpsc::channel();
        thread::spawn(move || searcher(query_rx, found_tx));
        ResultsLoader { queries, found }
    }

    /// Searches `query` in the background; the results come back in `found`.
    pub fn search(&self, query: &str) {
        // Fails only if the thread is gone; the status then stays on "searching".
        let _ = self.queries.send(query.to_string());
    }
}

/// One search at a time; queries already outdated when it is free are skipped.
/// Returns once a channel is closed, i.e. when the app quits.
fn searcher(queries: Receiver<String>, found: Sender<Found>) -> Option<()> {
    loop {
        let query = queries.recv().ok()?;
        let query = queries.try_iter().last().unwrap_or(query);
        let result = anime_sama::search(&query).map_err(|e| e.to_string());
        found.send(Found { query, result }).ok()?;
    }
}
