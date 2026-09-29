//! Background threads that download and decode the pages for the reading screen, with their cache; draws nothing.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::panic::catch_unwind;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use image::imageops::FilterType;

use crate::Result;
use crate::i18n::tf;
use crate::sources::anime_sama;

/// Pages decoded at the same time. Downloads stay one at a time.
const DECODERS: usize = 2;
/// Kitty refuses images over 10 000 px on either side.
const MAX_SIDE: u32 = 10_000;

/// What the reader wants now. Each plan replaces the previous one; an empty plan stops everything.
#[derive(Default)]
pub struct Plan {
    /// Tags the images sent back, so the reader knows which chapter they are for.
    pub job: u64,
    pub title: String,
    /// Pixel width to decode pages at.
    pub width: u32,
    /// Pages to download and decode, in order: (chapter number, page from 1).
    pub decode: Vec<(u32, u32)>,
    /// Pages to have in the cache only, after those: (chapter number, page from 1).
    pub download: Vec<(u32, u32)>,
}

/// A downloaded page, waiting for a decoder.
struct Task {
    /// The plan it was asked in: decoders skip it once a newer plan exists.
    plan: u64,
    job: u64,
    page: u32,
    width: u32,
    bytes: Arc<Vec<u8>>,
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

/// Every page downloaded this session, never evicted: (title, chapter number, page from 1).
type Cache = Mutex<HashMap<(String, u32, u32), Arc<Vec<u8>>>>;

/// One thread downloads what the latest plan asks for, `DECODERS` others decode it.
pub struct PageLoader {
    plans: Sender<(u64, Plan)>,
    tasks: Sender<Task>,
    pub done: Receiver<Loaded>,
    /// Number of the latest plan, shared with the decoders.
    latest: Arc<AtomicU64>,
    cache: Arc<Cache>,
    last_job: u64,
}

impl PageLoader {
    pub fn spawn() -> Self {
        let (plans, plan_rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let (tasks, task_rx) = mpsc::channel();
        let task_rx = Arc::new(Mutex::new(task_rx));
        let latest = Arc::new(AtomicU64::new(0));
        let cache = Arc::new(Cache::default());
        for _ in 0..DECODERS {
            let (task_rx, done, latest) = (task_rx.clone(), done_tx.clone(), latest.clone());
            thread::spawn(move || decoder(&task_rx, &done, &latest));
        }
        let (downloader_tasks, downloader_cache) = (tasks.clone(), cache.clone());
        thread::spawn(move || downloader(plan_rx, downloader_tasks, done_tx, &downloader_cache));
        PageLoader {
            plans,
            tasks,
            done,
            latest,
            cache,
            last_job: 0,
        }
    }

    /// A new id for the images of a chapter, unique for the whole session.
    pub fn new_job(&mut self) -> u64 {
        self.last_job += 1;
        self.last_job
    }

    /// Replaces what the loader works on: work for an older plan is dropped.
    pub fn plan(&self, mut plan: Plan) {
        let number = self.latest.fetch_add(1, Relaxed) + 1;
        // Pages already downloaded go straight to the decoders: they must not wait for the
        // downloader, which only reads a new plan between two downloads (and one can be slow).
        if let Ok(cache) = self.cache.lock() {
            plan.decode.retain(|&(chapter, page)| {
                let Some(bytes) = cache.get(&(plan.title.clone(), chapter, page)) else {
                    return true;
                };
                let (job, width, bytes) = (plan.job, plan.width, bytes.clone());
                let task = Task {
                    plan: number,
                    job,
                    page,
                    width,
                    bytes,
                };
                // Fails only if the decoders are gone; the page then stays loading.
                let _ = self.tasks.send(task);
                false
            });
        }
        // Fails only if the downloader is gone; the pages then stay loading.
        let _ = self.plans.send((number, plan));
    }
}

/// Walks the latest plan one download at a time, to stay polite with the site: the pages to
/// decode (handed to the decoders), then the pages to cache. A newer plan takes over between two
/// pages. Returns once a channel is closed, i.e. when the app quits.
fn downloader(
    plans: Receiver<(u64, Plan)>,
    tasks: Sender<Task>,
    done: Sender<Loaded>,
    cache: &Cache,
) -> Option<()> {
    let (mut number, mut plan) = (0, Plan::default());
    let mut step = 0;
    loop {
        let finished = step >= plan.decode.len() + plan.download.len();
        let first = if finished {
            Some(plans.recv().ok()?)
        } else {
            None
        };
        if let Some(newest) = first.into_iter().chain(plans.try_iter()).last() {
            (number, plan) = newest;
            step = 0;
            continue;
        }
        let (job, width) = (plan.job, plan.width);
        if let Some(&(chapter, page)) = plan.decode.get(step) {
            match fetch(cache, &plan.title, chapter, page) {
                Ok(bytes) => tasks
                    .send(Task {
                        plan: number,
                        job,
                        page,
                        width,
                        bytes,
                    })
                    .ok()?,
                Err(e) => done
                    .send(Loaded {
                        job,
                        page,
                        image: Err(e),
                    })
                    .ok()?,
            }
        } else if let Some(&(chapter, page)) = plan.download.get(step - plan.decode.len()) {
            // A failed download is tried again when the page has to be decoded, and shown then.
            let _ = fetch(cache, &plan.title, chapter, page);
        }
        step += 1;
    }
}

/// The page's bytes, from the cache, or downloaded into it. The lock is not held while
/// downloading, so `PageLoader::plan` can read the cache meanwhile.
fn fetch(cache: &Cache, title: &str, chapter: u32, page: u32) -> Result<Arc<Vec<u8>>, String> {
    let key = (title.to_string(), chapter, page);
    if let Some(bytes) = cache.lock().ok().and_then(|cache| cache.get(&key).cloned()) {
        return Ok(bytes);
    }
    let bytes = Arc::new(anime_sama::page(title, chapter, page).map_err(|e| e.to_string())?);
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, bytes.clone());
    }
    Ok(bytes)
}

/// Decodes the downloaded pages of the latest plan; those of an older plan are skipped (the
/// reader puts the ones it still needs in the new plan). Returns once a channel is closed.
fn decoder(tasks: &Mutex<Receiver<Task>>, done: &Sender<Loaded>, latest: &AtomicU64) -> Option<()> {
    loop {
        // The lock is only held while waiting for a task, not while decoding it.
        let task = tasks.lock().ok()?.recv().ok()?;
        if task.plan != latest.load(Relaxed) {
            continue;
        }
        // Odd image data that makes the decoder panic only fails this page.
        let image = catch_unwind(|| decode(&task.bytes, task.width))
            .unwrap_or_else(|_| Err(tf("page_loader.bad_image", &[("e", &"panic")])));
        let (job, page) = (task.job, task.page);
        done.send(Loaded { job, page, image }).ok()?;
    }
}

/// Decodes an image and resizes it to `width` px, keeping its aspect. A page so long that it
/// would be taller than kitty accepts is made narrower instead.
pub fn decode(bytes: &[u8], width: u32) -> Result<Image, String> {
    let image =
        image::load_from_memory(bytes).map_err(|e| tf("page_loader.bad_image", &[("e", &e)]))?;
    let widest = u64::from(image.width()) * u64::from(MAX_SIDE) / u64::from(image.height().max(1));
    let width = width.min(widest.max(1) as u32);
    // The reader shows pages 1:1. `thumbnail` is a fast shrink (each source pixel counts once),
    // several times quicker than a filtered resize.
    let image = match image.width().cmp(&width) {
        Ordering::Greater => image.thumbnail(width, MAX_SIDE),
        Ordering::Less => image.resize(width, MAX_SIDE, FilterType::Triangle),
        Ordering::Equal => image,
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

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let mut jpeg = Vec::new();
        DynamicImage::new_rgb8(width, height)
            .write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
            .unwrap();
        jpeg
    }

    #[test]
    fn decoded_at_column_width() {
        let page = jpeg(37, 53);
        for width in [20, 37, 900] {
            assert_eq!(decode(&page, width).unwrap().width, width);
        }
    }

    #[test]
    fn long_pages_fit_in_kitty() {
        // 900 px wide, 10×2000 would be 180 000 px tall: made 50×10 000 instead.
        let image = decode(&jpeg(10, 2000), 900).unwrap();
        assert_eq!((image.width, image.height), (50, MAX_SIDE));
    }
}
