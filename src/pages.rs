use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use image::imageops::FilterType;

use crate::Result;
use crate::http;
use crate::i18n::tf;
use crate::source::{Chapter, Source};

/// Pages of a chapter to download and decode for the reader, `start` first.
struct Show {
    id: u64,
    source: Source,
    chapter: Chapter,
    start: usize,
    width: u32,
}

enum Job {
    Show(Show),
    /// Chapters to download ahead into the cache, without decoding.
    Prefetch(Source, Vec<Chapter>),
}

/// What the worker sends back for the job `job`.
pub enum Loaded {
    /// The chapter's page count, before any of its pages.
    Count {
        job: u64,
        pages: Result<usize, String>,
    },
    Page {
        job: u64,
        page: usize,
        image: Result<Image, String>,
    },
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

    /// Asks for the pages of `chapter`, page `start` first. Returns the id that tags their results.
    pub fn request(&mut self, source: Source, chapter: &Chapter, start: usize, width: u32) -> u64 {
        self.last_job += 1;
        // Fails only if the worker is gone; the pages then stay loading.
        let _ = self.jobs.send(Job::Show(Show {
            id: self.last_job,
            source,
            chapter: chapter.clone(),
            start,
            width,
        }));
        self.last_job
    }

    /// The worker only works on the last chapter asked for: an older job gets nothing more.
    pub fn is_current(&self, job: u64) -> bool {
        job == self.last_job
    }

    /// Downloads these chapters into the cache when nothing is left to show.
    pub fn prefetch(&self, source: Source, chapters: &[Chapter]) {
        let _ = self.jobs.send(Job::Prefetch(source, chapters.to_vec()));
    }
}

/// One request at a time, to stay polite with the sites: the shown chapter first, then the prefetch.
/// A new job replaces the one of its kind between two requests.
fn work(jobs: Receiver<Job>, done: Sender<Loaded>) -> Option<()> {
    // ponytail: every page downloaded this session stays in memory; add an LRU if long sessions get heavy.
    let mut cache: HashMap<(String, usize), Vec<u8>> = HashMap::new();
    // The chapter asked for, until its page list is known.
    let mut asked: Option<Show> = None;
    // The chapter being shown, its image URLs, and its pages still to send (popped from the end).
    let mut show: Option<(Show, Vec<String>)> = None;
    let mut queue: Vec<usize> = Vec::new();
    // Chapters to download ahead (popped from the end), and the pages of the one in progress.
    let mut ahead_source = Source::AnimeSama;
    let mut ahead: Vec<Chapter> = Vec::new();
    let mut ahead_pages: Vec<(String, usize, String)> = Vec::new();
    loop {
        // Idle: wait for a job. Busy: only take the jobs already sent, between two requests.
        let idle =
            asked.is_none() && queue.is_empty() && ahead.is_empty() && ahead_pages.is_empty();
        let first = if idle { Some(jobs.recv().ok()?) } else { None };
        for job in first.into_iter().chain(jobs.try_iter()) {
            match job {
                Job::Show(new) => {
                    asked = Some(new);
                    queue.clear();
                }
                Job::Prefetch(source, chapters) => {
                    ahead_source = source;
                    ahead = chapters.into_iter().rev().collect();
                    ahead_pages.clear();
                }
            }
        }

        if let Some(new) = asked.take() {
            // The page list first (Mangas-Origines: one request for the chapter's page).
            let urls = new.source.pages(&new.chapter).map_err(|e| e.to_string());
            let count = urls.as_ref().map(Vec::len).map_err(String::clone);
            done.send(Loaded::Count {
                job: new.id,
                pages: count,
            })
            .ok()?;
            if let Ok(urls) = urls {
                let start = new.start.min(urls.len());
                // `start` first, the rest of the chapter, then the pages before it.
                queue = (start..urls.len()).chain(0..start).rev().collect();
                show = Some((new, urls));
            }
        } else if let (Some((current, urls)), Some(page)) = (&show, queue.pop()) {
            let bytes = match cache.entry((current.chapter.id.clone(), page)) {
                Entry::Occupied(entry) => Ok(entry.into_mut()),
                Entry::Vacant(entry) => http::bytes(&urls[page])
                    .map(|bytes| entry.insert(bytes))
                    .map_err(|e| e.to_string()),
            };
            let image = bytes.and_then(|bytes| decode(bytes, current.width));
            let job = current.id;
            done.send(Loaded::Page { job, page, image }).ok()?;
        } else if let Some((chapter, page, url)) = ahead_pages.pop() {
            // A failed prefetch is simply fetched again when shown.
            if let Entry::Vacant(entry) = cache.entry((chapter, page))
                && let Ok(bytes) = http::bytes(&url)
            {
                entry.insert(bytes);
            }
        } else if let Some(chapter) = ahead.pop()
            && let Ok(urls) = ahead_source.pages(&chapter)
        {
            let pages = urls.into_iter().enumerate().rev();
            ahead_pages = pages
                .map(|(page, url)| (chapter.id.clone(), page, url))
                .collect();
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
