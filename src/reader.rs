use std::collections::HashMap;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::widgets::{ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::Result;
use crate::i18n::{t, tf};
use crate::kitty::{self, Geo, Placement, Stored};
use crate::pages::{Loaded, Pages};
use crate::search::Opened;
use crate::source::{Chapter, Source};

/// Pages are at most this wide, like on the site.
const MAX_WIDTH_PX: u32 = 900;
/// Rows scrolled by j/k and by one mouse wheel notch.
const STEP_ROWS: u32 = 3;
/// After this many scrolls in a chapter, the chapters after the next one are downloaded ahead
/// (the next one is already loaded whole, see `prepare_next`).
const PREFETCH_AFTER: u32 = 3;
const PREFETCH_CHAPTERS: usize = 4;
/// Kitty keeps 320 MB of images and silently drops the oldest beyond: the chapters kept around the
/// current one must stay under this (counted as sent, 3 bytes per pixel).
const KITTY_BUDGET: u64 = 256 * 1024 * 1024;
/// Time constant of the scroll ease-out, like CSS `scroll-behavior: smooth`: 95 % done after 3 of them.
const GLIDE: Duration = Duration::from_millis(80);

#[derive(PartialEq)]
pub enum Exit {
    Quit,
    Back,
}

/// Reader sizes.
impl Geo {
    /// Width of the page column, in cells.
    fn page_cols(self) -> u16 {
        self.cols.min((MAX_WIDTH_PX / self.cell_w).max(1) as u16)
    }

    /// Width of the page column, in pixels: pages are decoded at this width and shown 1:1.
    fn page_px(self) -> u32 {
        u32::from(self.page_cols()) * self.cell_w
    }
}

/// Top of the view: a page and a pixel inside it, so pages loading above don't move what is read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pos {
    page: usize,
    px: u32,
}

/// Moves the view by `delta` px, never above the first page nor below the end line (`end` px high).
/// Works on the offset from the top of the chapter: `pos` → offset, add `delta` and clamp,
/// then walk the pages to turn the offset back into a page and a pixel inside it.
fn scroll(heights: &[u32], view: u32, end: u32, pos: Pos, delta: i64) -> Pos {
    let above: u32 = heights[..pos.page.min(heights.len())].iter().sum();
    let max = (heights.iter().sum::<u32>() + end).saturating_sub(view);
    let mut offset = (i64::from(above + pos.px) + delta).clamp(0, i64::from(max)) as u32;
    let mut page = 0;
    while page + 1 < heights.len() && offset >= heights[page] {
        offset -= heights[page];
        page += 1;
    }
    Pos { page, px: offset }
}

/// The slices of pages the view shows from `pos`, top to bottom: (page, first px, height px).
fn visible(heights: &[u32], view: u32, pos: Pos) -> Vec<(usize, u32, u32)> {
    let mut slices = Vec::new();
    let (mut page, mut top, mut left) = (pos.page, pos.px, view);
    while left > 0 && page < heights.len() {
        let height = heights[page].saturating_sub(top).min(left);
        if height > 0 {
            slices.push((page, top, height));
        }
        left -= height;
        page += 1;
        top = 0;
    }
    slices
}

/// The chapters to download ahead while reading chapter `current`: the ones after the next.
fn next_chapters(chapters: &[Chapter], current: usize) -> &[Chapter] {
    let start = (current + 2).min(chapters.len());
    &chapters[start..(start + PREFETCH_CHAPTERS).min(chapters.len())]
}

/// Part of the pending scroll to do after `dt`: by elapsed time, so the glide is the same at any
/// frame rate. At least 1 px, never past `pending`.
fn ease(pending: i64, dt: Duration) -> i64 {
    let share = 1.0 - (-dt.as_secs_f64() / GLIDE.as_secs_f64()).exp();
    match (pending as f64 * share).round() as i64 {
        // Rounds to 0 when little is left: move 1 px instead, so the glide always ends.
        0 => pending.signum(),
        step => step,
    }
}

enum Page {
    Loading,
    Failed(String),
    Ready(Stored),
}

/// Some pages are still on their way.
fn loading(pages: &[Page]) -> bool {
    pages.iter().any(|page| matches!(page, Page::Loading))
}

/// What these pages' images weigh in kitty.
fn stored_bytes(pages: &[Page]) -> u64 {
    let size = |page: &Page| match page {
        Page::Ready(image) => u64::from(image.width) * u64::from(image.height) * 3,
        _ => 0,
    };
    pages.iter().map(size).sum()
}

/// Deletes these pages' images from kitty.
fn free_pages(pages: &[Page]) -> Result<()> {
    kitty::free(pages.iter().filter_map(|page| match page {
        Page::Ready(image) => Some(image.id),
        _ => None,
    }))
}

/// The kept chapters to let go when opening `chapter` (`opened` bytes): the ones that aren't next
/// to it, and all of them when, with it, they weigh more than kitty's budget (long strips of huge
/// pages). `kept`: (chapter index, bytes in kitty).
fn to_drop(kept: &[(usize, u64)], chapter: usize, opened: u64) -> Vec<usize> {
    let near = |index: usize| index.abs_diff(chapter) <= 1;
    let weight: u64 = kept
        .iter()
        .filter(|(index, _)| near(*index))
        .map(|(_, size)| size)
        .sum();
    let heavy = weight + opened > KITTY_BUDGET;
    let dropped = kept.iter().filter(|(index, _)| heavy || !near(*index));
    dropped.map(|(index, _)| *index).collect()
}

/// The pages of a chapter next to the current one, kept in kitty so that going there is instant,
/// with the job that loads them.
struct Held {
    job: u64,
    pages: Vec<Page>,
}

enum Overlay {
    /// `filter`: the chapter number typed so far.
    Chapters {
        list: ListState,
        filter: String,
    },
    Help,
    Languages(ListState),
}

fn chapter_list(selected: usize) -> Overlay {
    let list = ListState::default().with_selected(Some(selected));
    Overlay::Chapters {
        list,
        filter: String::new(),
    }
}

/// Indexes of the chapters whose number starts with `filter`.
fn matching(chapters: &[Chapter], filter: &str) -> Vec<usize> {
    let starts = |c: &Chapter| c.label.starts_with(filter);
    (0..chapters.len())
        .filter(|&i| starts(&chapters[i]))
        .collect()
}

struct Reader<'a> {
    io: &'a mut Pages,
    source: Source,
    title: String,
    chapters: Vec<Chapter>,
    chapter: usize,
    job: u64,
    /// The current chapter's pages: one placeholder until their count is known, none before a
    /// chapter is chosen.
    pages: Vec<Page>,
    /// The previous and next chapters, by index, when they were loaded.
    kept: HashMap<usize, Held>,
    pos: Pos,
    /// Pixels still to scroll, done a bit each frame.
    pending: i64,
    /// Scroll actions in this chapter, to start the prefetch.
    scrolls: u32,
    frame_at: Instant,
    geo: Geo,
    /// Pixel heights of each page and of the view, from the last frame.
    heights: Vec<u32>,
    view: u32,
    bar: bool,
    overlay: Option<Overlay>,
}

pub fn run(terminal: &mut DefaultTerminal, io: &mut Pages, opened: Opened) -> Result<Exit> {
    let (source, title, chapters) = opened;
    let mut reader = Reader {
        io,
        source,
        title,
        chapters,
        chapter: 0,
        job: 0,
        pages: Vec::new(),
        kept: HashMap::new(),
        pos: Pos::default(),
        pending: 0,
        scrolls: 0,
        frame_at: Instant::now(),
        geo: Geo::now()?,
        heights: Vec::new(),
        view: 0,
        bar: true,
        overlay: Some(chapter_list(0)),
    };
    // Nothing is downloaded until a chapter is chosen in the list.
    let exit = reader.event_loop(terminal);
    reader.delete_images()?;
    exit
}

impl Reader<'_> {
    /// Goes to `chapter`. Kitty keeps the images of the chapters on each side of it, and only those.
    fn open(&mut self, chapter: usize) -> Result<()> {
        if !self.pages.is_empty() {
            let pages = std::mem::take(&mut self.pages);
            self.kept.insert(
                self.chapter,
                Held {
                    job: self.job,
                    pages,
                },
            );
        }
        self.chapter = chapter;
        self.pos = Pos::default();
        self.pending = 0;
        self.scrolls = 0;
        let opened = self.kept.remove(&chapter);
        let sizes: Vec<(usize, u64)> = self
            .kept
            .iter()
            .map(|(index, held)| (*index, stored_bytes(&held.pages)))
            .collect();
        let opened_size = opened.as_ref().map_or(0, |held| stored_bytes(&held.pages));
        let dropped = to_drop(&sizes, chapter, opened_size);
        for index in dropped {
            if let Some(held) = self.kept.remove(&index) {
                free_pages(&held.pages)?;
            }
        }
        match opened {
            Some(held) => {
                self.job = held.job;
                self.pages = held.pages;
                // Half loaded, and the worker has moved on to another chapter since: ask again.
                if loading(&self.pages) && !self.io.is_current(self.job) {
                    self.load();
                }
            }
            None => self.load(),
        }
        Ok(())
    }

    /// Asks for the current chapter's pages at the current width, from the visible page on.
    fn load(&mut self) {
        if self.pages.is_empty() {
            // One placeholder until the worker tells how many pages there are.
            self.pages.push(Page::Loading);
        }
        let chapter = &self.chapters[self.chapter];
        let width = self.geo.page_px();
        self.job = self.io.request(self.source, chapter, self.pos.page, width);
    }

    /// Once the current chapter is fully loaded, loads the next one as well: `l` then shows it at once.
    fn prepare_next(&mut self) {
        let next = self.chapter + 1;
        if self.pages.is_empty()
            || loading(&self.pages)
            || next >= self.chapters.len()
            || self.kept.contains_key(&next)
        {
            return;
        }
        // Only if a chapter like this one still fits in kitty next to what's there.
        let kept: u64 = self
            .kept
            .values()
            .map(|held| stored_bytes(&held.pages))
            .sum();
        if kept + 2 * stored_bytes(&self.pages) > KITTY_BUDGET {
            return;
        }
        let width = self.geo.page_px();
        let job = self.io.request(self.source, &self.chapters[next], 0, width);
        let pages = vec![Page::Loading];
        self.kept.insert(next, Held { job, pages });
    }

    /// Frees every image the reader put in kitty: the current chapter's and the kept ones.
    fn delete_images(&mut self) -> Result<()> {
        free_pages(&self.pages)?;
        for (_, held) in self.kept.drain() {
            free_pages(&held.pages)?;
        }
        Ok(())
    }

    /// Stores what the worker sent in the current chapter or in a kept one; images go to kitty.
    fn receive(&mut self, loaded: Loaded) -> Result<()> {
        let (Loaded::Count { job, .. } | Loaded::Page { job, .. }) = &loaded;
        let job = *job;
        let pages = if job == self.job {
            &mut self.pages
        } else if let Some(held) = self.kept.values_mut().find(|held| held.job == job) {
            &mut held.pages
        } else {
            return Ok(());
        };
        match loaded {
            // A kept chapter asked again already has its pages: keep the ones that are ready.
            Loaded::Count {
                pages: Ok(count), ..
            } if pages.len() != count => *pages = (0..count).map(|_| Page::Loading).collect(),
            Loaded::Count { pages: Err(e), .. } => *pages = vec![Page::Failed(e)],
            Loaded::Count { .. } => {}
            Loaded::Page { page, image, .. } => {
                if let Some(slot) = pages.get_mut(page)
                    && matches!(slot, Page::Loading)
                {
                    *slot = match image {
                        Ok(image) => Page::Ready(kitty::transmit(&image)?),
                        Err(e) => Page::Failed(e),
                    };
                }
            }
        }
        Ok(())
    }

    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<Exit> {
        // What kitty shows now.
        let mut shown = Vec::new();
        loop {
            while let Ok(loaded) = self.io.done.try_recv() {
                self.receive(loaded)?;
            }
            self.prepare_next();
            let geo = Geo::now()?;
            if geo != self.geo {
                // Resizing clears the screen, which drops kitty's images: send them again at the new width.
                self.geo = geo;
                if !self.pages.is_empty() {
                    self.delete_images()?;
                    self.pages.clear();
                    self.load();
                }
            }
            let now = Instant::now();
            // Capped, so that the time spent waiting for a key doesn't turn into a jump.
            self.animate(
                now.duration_since(self.frame_at)
                    .min(Duration::from_millis(16)),
            );
            self.frame_at = now;
            kitty::frame(terminal, &mut shown, |frame| self.draw(frame))?;
            // ~120 fps while scrolling. Drain the whole burst (mouse wheel) before drawing again.
            let mut timeout = Duration::from_millis(if self.pending == 0 { 50 } else { 8 });
            while event::poll(timeout)? {
                if let Some(exit) = self.handle(event::read()?)? {
                    return Ok(exit);
                }
                timeout = Duration::ZERO;
            }
        }
    }

    fn animate(&mut self, dt: Duration) {
        if self.pending == 0 {
            return;
        }
        let step = ease(self.pending, dt);
        let pos = self.scrolled(step);
        if pos == self.pos {
            // Top or bottom reached.
            self.pending = 0;
        } else {
            self.pos = pos;
            self.pending -= step;
        }
    }

    /// Draws the text and returns where kitty must draw the images.
    fn draw(&mut self, frame: &mut Frame) -> Vec<Placement> {
        let [main, bar] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(u16::from(self.bar))])
                .areas(frame.area());
        self.layout(main);
        let placements = self.draw_pages(frame, main);
        if self.bar {
            self.draw_bar(frame, bar);
        }
        match &mut self.overlay {
            None => return placements,
            Some(Overlay::Chapters { list, filter }) => {
                let items = matching(&self.chapters, filter)
                    .into_iter()
                    .map(|i| tf("reader.chapter", &[("n", &self.chapters[i].label)]))
                    .collect();
                let title = format!("{} {filter}", t("reader.chapters"));
                crate::popup(frame, title.trim_end(), items, list);
            }
            Some(Overlay::Languages(list)) => crate::draw_languages(frame, list),
            Some(Overlay::Help) => {
                let items = t("reader.help").lines().map(String::from).collect();
                crate::popup(frame, t("help"), items, &mut ListState::default());
            }
        }
        // Images are drawn above the text: hide them while a popup is open.
        Vec::new()
    }

    /// Page heights and view size for this frame, keeping the view inside the chapter.
    fn layout(&mut self, main: Rect) {
        let placeholder = self.geo.page_px() * 3 / 2;
        self.heights = self
            .pages
            .iter()
            .map(|page| match page {
                Page::Ready(image) => image.height,
                _ => placeholder,
            })
            .collect();
        self.view = u32::from(main.height) * self.geo.cell_h;
        // Moves nothing: only brings `pos` back inside the chapter when heights changed
        // (a page loaded, a resize).
        self.pos = self.scrolled(0);
    }

    /// The position `delta` px away, kept inside the chapter. Below the last page comes the
    /// "end of chapter" line, one cell high: that's the `end` given to `scroll`.
    fn scrolled(&self, delta: i64) -> Pos {
        let end_line = self.geo.cell_h;
        scroll(&self.heights, self.view, end_line, self.pos, delta)
    }

    /// Draws the loading and error messages and the end line; returns where the page images go.
    fn draw_pages(&self, frame: &mut Frame, main: Rect) -> Vec<Placement> {
        if self.pages.is_empty() {
            // No chapter chosen yet.
            return Vec::new();
        }
        let geo = self.geo;
        let cols = geo.page_cols().min(main.width);
        let x = main.x + (main.width - cols) / 2;
        // Pixels from the top of `main`.
        let mut y = 0;
        let mut placements = Vec::new();
        for (page, top, height) in visible(&self.heights, self.view, self.pos) {
            let row = (y / geo.cell_h) as u16;
            let text = match &self.pages[page] {
                Page::Ready(image) => {
                    placements.push(Placement {
                        id: image.id,
                        x,
                        y: main.y + row,
                        offset: y % geo.cell_h,
                        src_y: top,
                        src_w: image.width,
                        src_h: height,
                    });
                    None
                }
                Page::Loading => Some(t("loading").to_string()),
                Page::Failed(e) => Some(tf("error", &[("e", e)])),
            };
            if let Some(text) = text {
                let rows = (y + height).div_ceil(geo.cell_h) as u16 - row;
                message(frame, Rect::new(x, main.y + row, cols, rows), &text);
            }
            y += height;
        }

        let end_row = y.div_ceil(geo.cell_h) as u16;
        if end_row < main.height {
            let end = match self.chapters.get(self.chapter + 1) {
                Some(_) => tf("reader.end", &[("n", &self.chapters[self.chapter].label)]),
                None => t("reader.last").to_string(),
            };
            let line = Rect::new(main.x, main.y + end_row, main.width, 1);
            frame.render_widget(Paragraph::new(end).centered().dim(), line);
        }
        placements
    }

    fn draw_bar(&self, frame: &mut Frame, area: Rect) {
        let status = tf(
            "reader.status",
            &[
                ("title", &self.title),
                ("chapter", &self.chapters[self.chapter].label),
                ("chapters", &self.chapters.len()),
                ("page", &(self.pos.page + 1)),
                ("pages", &self.pages.len()),
            ],
        );
        frame.render_widget(Paragraph::new(status).reversed(), area);
    }

    fn handle(&mut self, event: Event) -> Result<Option<Exit>> {
        let code = match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match key.code {
                        KeyCode::Char('c') => return Ok(Some(Exit::Quit)),
                        KeyCode::Char('l') => self.toggle_languages(),
                        _ => {}
                    }
                    return Ok(None);
                }
                key.code
            }
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::ScrollDown => KeyCode::Down,
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::ScrollUp => KeyCode::Up,
            // Left click: previous chapter, right click: next one, like h and l.
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                KeyCode::Left
            }
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Right) => {
                KeyCode::Right
            }
            _ => return Ok(None),
        };
        let step = i64::from(STEP_ROWS * self.geo.cell_h);
        let half = i64::from(self.view / 2);
        match code {
            KeyCode::Char('q') => return Ok(Some(Exit::Quit)),
            // Back to the search from anywhere, unless it erases a typed chapter number.
            KeyCode::Backspace if !self.typing() => return Ok(Some(Exit::Back)),
            KeyCode::F(2) => self.bar = !self.bar,
            _ if self.overlay.is_some() => self.handle_overlay(code)?,
            KeyCode::Esc => return Ok(Some(Exit::Back)),
            KeyCode::F(1) => self.overlay = Some(chapter_list(self.chapter)),
            KeyCode::Char('?') => self.overlay = Some(Overlay::Help),
            KeyCode::Char('j') | KeyCode::Down => self.scroll_by(step),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_by(-step),
            KeyCode::Char('d') => self.scroll_by(half),
            KeyCode::Char('u') => self.scroll_by(-half),
            KeyCode::Char('l') | KeyCode::Right if self.chapter + 1 < self.chapters.len() => {
                self.open(self.chapter + 1)?
            }
            KeyCode::Char('h') | KeyCode::Left if self.chapter > 0 => {
                self.open(self.chapter - 1)?
            }
            _ => {}
        }
        Ok(None)
    }

    fn handle_overlay(&mut self, code: KeyCode) -> Result<()> {
        if let Some(Overlay::Languages(list)) = &mut self.overlay {
            if !crate::pick_language(list, code) {
                self.toggle_languages();
            }
            return Ok(());
        }
        let Some(Overlay::Chapters { list, filter }) = &mut self.overlay else {
            // Any key closes the help.
            self.overlay = None;
            return Ok(());
        };
        match code {
            KeyCode::Char(digit) if digit.is_ascii_digit() => {
                filter.push(digit);
                list.select(Some(0));
            }
            KeyCode::Backspace => {
                filter.pop();
                list.select(Some(0));
            }
            KeyCode::Char('j') | KeyCode::Down => list.select_next(),
            KeyCode::Char('k') | KeyCode::Up => list.select_previous(),
            KeyCode::Enter => {
                let shown = matching(&self.chapters, filter);
                // ListState only clamps its selection when rendered.
                let selected = list
                    .selected()
                    .unwrap_or(0)
                    .min(shown.len().saturating_sub(1));
                let Some(&chapter) = shown.get(selected) else {
                    return Ok(());
                };
                self.overlay = None;
                self.open(chapter)?;
            }
            KeyCode::F(1) | KeyCode::Esc => {
                self.overlay = None;
                // Closed without choosing: read the current chapter.
                if self.pages.is_empty() {
                    self.load();
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Opens or closes the language list. Closed before a chapter is chosen, it gives the chapter list back.
    fn toggle_languages(&mut self) {
        self.overlay = match self.overlay {
            Some(Overlay::Languages(_)) if self.pages.is_empty() => Some(chapter_list(0)),
            Some(Overlay::Languages(_)) => None,
            _ => Some(Overlay::Languages(crate::language_list())),
        };
    }

    /// A chapter number is being typed in the chapter list.
    fn typing(&self) -> bool {
        matches!(&self.overlay, Some(Overlay::Chapters { filter, .. }) if !filter.is_empty())
    }

    fn scroll_by(&mut self, delta: i64) {
        self.pending += delta;
        self.scrolls += 1;
        if self.scrolls == PREFETCH_AFTER {
            let ahead = next_chapters(&self.chapters, self.chapter);
            self.io.prefetch(self.source, ahead);
        }
    }
}

fn message(frame: &mut Frame, area: Rect, text: &str) {
    let text = Paragraph::new(text).centered().wrap(Wrap { trim: true });
    frame.render_widget(text, area.centered_vertically(Constraint::Length(3)));
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEIGHTS: [u32; 3] = [100, 200, 50];
    const END: u32 = 10;

    fn at(page: usize, px: u32) -> Pos {
        Pos { page, px }
    }

    #[test]
    fn scrolling() {
        assert_eq!(scroll(&HEIGHTS, 80, END, at(0, 0), 30), at(0, 30));
        assert_eq!(scroll(&HEIGHTS, 80, END, at(0, 90), 30), at(1, 20));
        assert_eq!(scroll(&HEIGHTS, 80, END, at(1, 20), -30), at(0, 90));
        assert_eq!(scroll(&HEIGHTS, 80, END, at(0, 10), -50), at(0, 0));
        // 350 px of pages + the end line, in a view of 80: the bottom is 280 px down.
        assert_eq!(scroll(&HEIGHTS, 80, END, at(2, 0), 1000), at(1, 180));
        assert_eq!(scroll(&[30], 80, END, at(0, 0), 50), at(0, 0));
    }

    #[test]
    fn visible_slices() {
        assert_eq!(visible(&HEIGHTS, 80, at(0, 70)), [(0, 70, 30), (1, 0, 50)]);
        assert_eq!(
            visible(&HEIGHTS, 80, at(1, 180)),
            [(1, 180, 20), (2, 0, 50)]
        );
    }

    #[test]
    fn easing() {
        // After one time constant: 1 - e^-1 of the way.
        assert_eq!(ease(100, GLIDE), 63);
        assert_eq!(ease(-100, GLIDE), -63);
        assert_eq!(ease(3, Duration::from_millis(1)), 1);
        assert_eq!(ease(-1, Duration::from_millis(1)), -1);
        assert_eq!(ease(100, Duration::from_secs(10)), 100);
    }

    fn chapters(labels: &[&str]) -> Vec<Chapter> {
        let chapter = |label: &&str| Chapter {
            id: format!("/{label}"),
            label: label.to_string(),
        };
        labels.iter().map(chapter).collect()
    }

    #[test]
    fn prefetched_chapters() {
        let chapters = chapters(&["1", "2", "3", "4", "5", "6", "7"]);
        let labels = |current| {
            let ahead = next_chapters(&chapters, current);
            ahead.iter().map(|c| c.label.as_str()).collect::<Vec<_>>()
        };
        // The next one is loaded whole instead: the prefetch starts after it.
        assert_eq!(labels(0), ["3", "4", "5", "6"]);
        assert_eq!(labels(4), ["7"]);
        assert!(labels(5).is_empty());
    }

    #[test]
    fn chapters_kept_in_kitty() {
        const MB: u64 = 1024 * 1024;
        let mut dropped = to_drop(&[(3, 50 * MB), (5, 50 * MB), (8, MB)], 4, 50 * MB);
        dropped.sort();
        // Opening 4: 3 and 5 stay, 8 is too far.
        assert_eq!(dropped, [8]);
        // Too heavy together: nothing stays around.
        let mut dropped = to_drop(&[(3, 200 * MB), (5, MB)], 4, 100 * MB);
        dropped.sort();
        assert_eq!(dropped, [3, 5]);
    }

    #[test]
    fn chapter_filter() {
        let chapters = chapters(&["1", "2", "12", "21", "120", "12.5"]);
        assert_eq!(matching(&chapters, ""), [0, 1, 2, 3, 4, 5]);
        assert_eq!(matching(&chapters, "12"), [2, 4, 5]);
        assert!(matching(&chapters, "9").is_empty());
    }

    #[test]
    fn page_width() {
        let geo = Geo {
            cols: 200,
            rows: 50,
            cell_w: 10,
            cell_h: 20,
        };
        assert_eq!(geo.page_cols(), 90);
        assert_eq!(geo.page_px(), 900);
    }
}
