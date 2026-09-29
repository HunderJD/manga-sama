//! Background thread that downloads and decodes the search covers, with their cache; draws nothing.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::reader::page_loader::{self, Image};
use crate::sources::anime_sama;

/// A decoded cover.
pub struct Cover {
    pub url: String,
    /// The pixel width it was decoded at.
    pub width: u32,
    pub image: Result<Image, String>,
}

pub struct CoverLoader {
    jobs: Sender<(Vec<String>, u32)>,
    pub done: Receiver<Cover>,
}

impl CoverLoader {
    pub fn spawn() -> Self {
        let (jobs, job_rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        thread::spawn(move || loader(job_rx, done_tx));
        CoverLoader { jobs, done }
    }

    /// Gets these covers, in order, decoded `width` px wide. Replaces the previous request.
    pub fn request(&self, urls: Vec<String>, width: u32) {
        // Fails only if the thread is gone; the tiles then stay without covers.
        let _ = self.jobs.send((urls, width));
    }
}

/// Downloads and decodes covers, one request at a time. A new request replaces the queue.
/// Returns once a channel is closed, i.e. when the app quits.
fn loader(jobs: Receiver<(Vec<String>, u32)>, done: Sender<Cover>) -> Option<()> {
    // Every cover downloaded this session stays here (a few dozen KB each).
    let mut cache: HashMap<String, Vec<u8>> = HashMap::new();
    let mut queue: Vec<String> = Vec::new();
    let mut width = 0;
    loop {
        // Idle: wait for a request. Busy: only take the newest request already sent.
        let job = if queue.is_empty() {
            Some(jobs.recv().ok()?)
        } else {
            jobs.try_iter().last()
        };
        if let Some((urls, new_width)) = job {
            // Popped from the end: the first tile first.
            queue = urls.into_iter().rev().collect();
            width = new_width;
        }
        let Some(url) = queue.pop() else {
            continue;
        };
        let bytes = match cache.entry(url.clone()) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => anime_sama::cover(&url)
                .map(|bytes| entry.insert(bytes))
                .map_err(|e| e.to_string()),
        };
        let image = bytes.and_then(|bytes| page_loader::decode(bytes, width));
        done.send(Cover { url, width, image }).ok()?;
    }
}
