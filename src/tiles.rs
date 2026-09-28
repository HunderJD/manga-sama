//! Search results as a grid of covers, loaded like the site's `loading="lazy"`:
//! only the covers on screen are downloaded, one at a time.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::widgets::{Paragraph, Wrap};

use crate::Result;
use crate::api::{self, Link};
use crate::kitty::{self, Geo, Placement, Stored};
use crate::pages::{self, Image};

/// Tile width in cells. The cover's height follows the thumbnails' 440×248 aspect.
const TILE_COLS: u16 = 18;
const GAP: u16 = 1;

/// Covers to fetch (the missing ones on screen, in order) and the width to decode them at.
type Job = (Vec<String>, u32);
/// A decoded cover: its URL, the width it was decoded at, the image.
type Cover = (String, u32, Result<Image, String>);

pub struct Tiles {
    jobs: Sender<Job>,
    done: Receiver<Cover>,
    /// Covers already in kitty by URL; `None` when the download or decoding failed.
    thumbs: HashMap<String, Option<Stored>>,
    /// Covers of the last job sent.
    asked: Vec<String>,
    /// First grid row on screen.
    top: usize,
    /// Tiles per row, from the last frame: ↑ ↓ move by this much.
    pub columns: usize,
    /// Pixel width covers are decoded at, from the last frame.
    width: u32,
}

impl Tiles {
    pub fn new() -> Self {
        let (jobs, job_rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        thread::spawn(move || fetch(job_rx, done_tx));
        Tiles {
            jobs,
            done,
            thumbs: HashMap::new(),
            asked: Vec::new(),
            top: 0,
            columns: 1,
            width: 0,
        }
    }

    /// Sends the covers that arrived to kitty.
    pub fn receive(&mut self) -> Result<()> {
        while let Ok((url, width, image)) = self.done.try_recv() {
            // Decoded for an older size, or twice: skip it.
            if width != self.width || self.thumbs.contains_key(&url) {
                continue;
            }
            let stored = image
                .ok()
                .map(|image| kitty::transmit(&image))
                .transpose()?;
            self.thumbs.insert(url, stored);
        }
        Ok(())
    }

    /// Forgets the covers sent to kitty and frees them there: after a resize (which clears
    /// kitty's images) and when leaving the search. They come back from the RAM cache.
    pub fn reset(&mut self) -> Result<()> {
        kitty::free(self.thumbs.values().flatten().map(|image| image.id))?;
        self.thumbs.clear();
        self.asked.clear();
        Ok(())
    }

    /// Draws the grid with `selected` highlighted, asks for the missing covers on screen,
    /// and returns where kitty must draw the covers it has.
    pub fn draw(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        links: &[Link],
        selected: usize,
        geo: Geo,
    ) -> Vec<Placement> {
        let cover_rows = cover_rows(geo);
        let tile_rows = cover_rows + 1; // + the title
        self.columns = usize::from((area.width + GAP) / (TILE_COLS + GAP)).max(1);
        let rows = usize::from((area.height + GAP) / (tile_rows + GAP)).max(1);
        self.width = u32::from(TILE_COLS) * geo.cell_w;

        // Scroll so that the selected tile stays on screen.
        let row = selected / self.columns;
        if row < self.top {
            self.top = row;
        }
        if row >= self.top + rows {
            self.top = row + 1 - rows;
        }

        let first = self.top * self.columns;
        let last = (first + rows * self.columns).min(links.len());
        let mut placements = Vec::new();
        let mut missing = Vec::new();
        for (index, link) in links.iter().enumerate().take(last).skip(first) {
            let n = index - first;
            let x = area.x + (n % self.columns) as u16 * (TILE_COLS + GAP);
            let y = area.y + (n / self.columns) as u16 * (tile_rows + GAP);

            let title: String = link.name.chars().take(usize::from(TILE_COLS)).collect();
            let title = match index == selected {
                true => Paragraph::new(title).reversed(),
                false => Paragraph::new(title),
            };
            let title_area = Rect::new(x, y + cover_rows, TILE_COLS, 1).intersection(area);
            frame.render_widget(title, title_area);

            // (cover URL, what `thumbs` knows about it)
            match link.cover.as_ref().map(|url| (url, self.thumbs.get(url))) {
                // In kitty: place it.
                Some((_, Some(Some(image)))) => placements.push(Placement {
                    id: image.id,
                    x,
                    y,
                    offset: 0,
                    src_y: 0,
                    src_w: image.width,
                    src_h: image.height.min(u32::from(cover_rows) * geo.cell_h),
                }),
                // Not downloaded yet: ask for it.
                Some((url, None)) => missing.push(url.clone()),
                // No cover, or it failed: the name in its place.
                _ => {
                    let name = Paragraph::new(link.name.as_str()).wrap(Wrap { trim: true });
                    let cover_area = Rect::new(x, y, TILE_COLS, cover_rows).intersection(area);
                    frame.render_widget(name.dim(), cover_area);
                }
            }
        }

        // Only when a cover on screen was never asked for: arrivals alone don't resend anything.
        if missing.iter().any(|url| !self.asked.contains(url)) {
            // Fails only if the cover thread is gone; the tiles then stay without covers.
            let _ = self.jobs.send((missing.clone(), self.width));
            self.asked = missing;
        }
        placements
    }
}

/// Rows taken by a cover `TILE_COLS` wide, from the thumbnails' 440×248 aspect.
fn cover_rows(geo: Geo) -> u16 {
    let px = u32::from(TILE_COLS) * geo.cell_w * 248 / 440;
    ((px + geo.cell_h / 2) / geo.cell_h).max(1) as u16
}

/// The tile selected after an arrow key, in a grid of `columns`.
pub fn moved(selected: usize, len: usize, columns: usize, key: KeyCode) -> usize {
    let target = match key {
        KeyCode::Left => selected.checked_sub(1),
        KeyCode::Right => Some(selected + 1),
        KeyCode::Up => selected.checked_sub(columns),
        KeyCode::Down => Some(selected + columns),
        _ => None,
    };
    target.filter(|&i| i < len).unwrap_or(selected)
}

/// Downloads and decodes covers, one request at a time. A new job replaces the queue.
fn fetch(jobs: Receiver<Job>, done: Sender<Cover>) -> Option<()> {
    // ponytail: every cover downloaded this session stays in memory (a few dozen KB each).
    let mut cache: HashMap<String, Vec<u8>> = HashMap::new();
    let mut queue: Vec<String> = Vec::new();
    let mut width = 0;
    loop {
        // Idle: wait for a job. Busy: only take the newest job already sent.
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
            Entry::Vacant(entry) => api::cover(&url)
                .map(|bytes| entry.insert(bytes))
                .map_err(|e| e.to_string()),
        };
        let image = bytes.and_then(|bytes| pages::decode(bytes, width));
        done.send((url, width, image)).ok()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid() {
        // 7 results in rows of 3.
        assert_eq!(moved(0, 7, 3, KeyCode::Right), 1);
        assert_eq!(moved(0, 7, 3, KeyCode::Left), 0);
        assert_eq!(moved(1, 7, 3, KeyCode::Down), 4);
        assert_eq!(moved(4, 7, 3, KeyCode::Down), 4);
        assert_eq!(moved(4, 7, 3, KeyCode::Up), 1);
        assert_eq!(moved(6, 7, 3, KeyCode::Right), 6);
        // 18 cells of 10×20 px: 180×101 px, about 5 rows.
        let geo = Geo {
            cols: 200,
            rows: 50,
            cell_w: 10,
            cell_h: 20,
        };
        assert_eq!(cover_rows(geo), 5);
    }
}
