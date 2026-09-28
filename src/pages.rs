use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use image::imageops::FilterType;

use crate::Result;
use crate::api::{self, Chapter};
use crate::i18n::tf;

/// Pages of a chapter to download and decode for the reader, `start` first.
struct Show {
    id: u64,
    title: String,
    chapter: Chapter,
    start: u32,
    width: u32,
}

enum Job {
    Show(Show),
    /// Chapters to download ahead into the cache, without decoding.
    Prefetch(String, Vec<Chapter>),
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

/// Background worker that downloads and decodes the pages of the requested chapter,
/// then downloads the chapters asked for ahead.
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
        // Fails only if the worker is gone; the pages then stay loading.
        let _ = self.jobs.send(Job::Show(Show {
            id: self.last_job,
            title: title.to_string(),
            chapter,
            start,
            width,
        }));
        self.last_job
    }

    /// Downloads these chapters into the cache when nothing is left to show.
    pub fn prefetch(&self, title: &str, chapters: &[Chapter]) {
        let _ = self
            .jobs
            .send(Job::Prefetch(title.to_string(), chapters.to_vec()));
    }
}

/// One request at a time, to stay polite with the site: the shown chapter first, then the prefetch.
/// A new job replaces the queue of its kind between two pages.
fn work(jobs: Receiver<Job>, done: Sender<Loaded>) -> Option<()> {
    // ponytail: every page downloaded this session stays in memory; add an LRU if long sessions get heavy.
    let mut cache: HashMap<(String, u32, u32), Vec<u8>> = HashMap::new();
    let mut show = None;
    let mut queue: Vec<u32> = Vec::new();
    let mut ahead_title = String::new();
    let mut ahead: Vec<(u32, u32)> = Vec::new();
    loop {
        // Idle: wait for a job. Busy: only take the jobs already sent, between two pages.
        let first = if queue.is_empty() && ahead.is_empty() {
            Some(jobs.recv().ok()?)
        } else {
            None
        };
        // Queues are popped from the end.
        for job in first.into_iter().chain(jobs.try_iter()) {
            match job {
                Job::Show(new) => {
                    // `start` first, the rest of the chapter, then the pages before it.
                    queue = (new.start..=new.chapter.pages)
                        .chain(1..new.start)
                        .rev()
                        .collect();
                    show = Some(new);
                }
                Job::Prefetch(title, chapters) => {
                    ahead = chapters
                        .iter()
                        .rev()
                        .flat_map(|c| (1..=c.pages).rev().map(|page| (c.number, page)))
                        .collect();
                    ahead_title = title;
                }
            }
        }

        if let (Some(current), Some(page)) = (&show, queue.pop()) {
            let number = current.chapter.number;
            let bytes = match cache.entry((current.title.clone(), number, page)) {
                Entry::Occupied(entry) => Ok(entry.into_mut()),
                Entry::Vacant(entry) => api::page(&current.title, number, page)
                    .map(|bytes| entry.insert(bytes))
                    .map_err(|e| e.to_string()),
            };
            let image = bytes.and_then(|bytes| decode(bytes, current.width));
            let job = current.id;
            done.send(Loaded { job, page, image }).ok()?;
        } else if let Some((chapter, page)) = ahead.pop()
            && let Entry::Vacant(entry) = cache.entry((ahead_title.clone(), chapter, page))
            // A failed prefetch is simply fetched again when shown.
            && let Ok(bytes) = api::page(&ahead_title, chapter, page)
        {
            entry.insert(bytes);
        }
    }
}

/// Decodes an image and resizes it to exactly `width` px, keeping its aspect.
pub fn decode(bytes: &[u8], width: u32) -> Result<Image, String> {
    let image = image::load_from_memory(bytes).map_err(|e| tf("pages.bad_image", &[("e", &e)]))?;
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat};

    use super::*;

    #[test]
    fn decoded_at_column_width() {
        let mut jpeg = Vec::new();
        DynamicImage::new_rgb8(37, 53)
            .write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
            .unwrap();
        for width in [20, 37, 900] {
            assert_eq!(decode(&jpeg, width).unwrap().width, width);
        }
    }
}
