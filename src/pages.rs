use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use image::imageops::FilterType;

use crate::Result;
use crate::api::{self, Chapter};

struct Job {
    id: u64,
    title: String,
    chapter: u32,
    pages: u32,
    start: u32,
    width: u32,
}

pub struct Loaded {
    pub job: u64,
    pub page: u32,
    pub image: Result<Image, String>,
}

pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// Background worker that downloads and decodes the pages of the requested chapter.
pub struct Pages {
    jobs: Sender<Job>,
    pub done: Receiver<Loaded>,
    last_job: u64,
}

impl Pages {
    pub fn spawn() -> Self {
        let (jobs, job_rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        thread::spawn(move || work(job_rx, done_tx));
        Pages {
            jobs,
            done,
            last_job: 0,
        }
    }

    /// Asks for the pages of `chapter`, `start` first. Returns the id that tags their results.
    pub fn request(&mut self, title: &str, chapter: Chapter, start: u32, width: u32) -> u64 {
        self.last_job += 1;
        // Fails only if the worker is gone; the pages then stay "Chargement…".
        let _ = self.jobs.send(Job {
            id: self.last_job,
            title: title.to_string(),
            chapter: chapter.number,
            pages: chapter.pages,
            start,
            width,
        });
        self.last_job
    }
}

/// One page at a time, to stay polite with the site. A new job replaces the current one between two pages.
fn work(jobs: Receiver<Job>, done: Sender<Loaded>) {
    // ponytail: every page downloaded this session stays in memory; add an LRU if long sessions get heavy.
    let mut cache: HashMap<(String, u32, u32), Vec<u8>> = HashMap::new();
    let mut job = None;
    let mut queue: Vec<u32> = Vec::new();
    loop {
        let newest = if queue.is_empty() {
            match jobs.recv() {
                Ok(job) => Some(job),
                Err(_) => return,
            }
        } else {
            jobs.try_iter().last()
        };
        if let Some(new) = newest {
            // Popped from the end: `start` first, the rest of the chapter, then the pages before it.
            queue = (new.start..=new.pages).chain(1..new.start).rev().collect();
            job = Some(new);
        }
        let (Some(current), Some(page)) = (&job, queue.pop()) else {
            continue;
        };
        let bytes = match cache.entry((current.title.clone(), current.chapter, page)) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => api::page(&current.title, current.chapter, page)
                .map(|bytes| entry.insert(bytes))
                .map_err(|e| e.to_string()),
        };
        let image = bytes.and_then(|bytes| decode(bytes, current.width));
        let loaded = Loaded {
            job: current.id,
            page,
            image,
        };
        if done.send(loaded).is_err() {
            return;
        }
    }
}

fn decode(bytes: &[u8], width: u32) -> Result<Image, String> {
    let image = image::load_from_memory(bytes).map_err(|e| format!("image illisible : {e}"))?;
    // Exactly the column width, up or down: the reader shows pages 1:1.
    let image = if image.width() == width {
        image
    } else {
        image.resize(width, u32::MAX, FilterType::Triangle)
    };
    let rgb = image.into_rgb8();
    Ok(Image {
        width: rgb.width(),
        height: rgb.height(),
        rgb: rgb.into_raw(),
    })
}
