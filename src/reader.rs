use std::io::stdout;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::widgets::{ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::Result;
use crate::api::Chapter;
use crate::i18n::{t, tf};
use crate::kitty::{self, Geo, Placement, Stored};
use crate::pages::{Loaded, Pages};

/// Pages are at most this wide, like on the site.
const MAX_WIDTH_PX: u32 = 900;
/// Rows scrolled by j/k and by one mouse wheel notch.
const STEP_ROWS: u32 = 3;
/// After this many scrolls in a chapter, the next chapters are downloaded ahead.
const PREFETCH_AFTER: u32 = 3;
const PREFETCH_CHAPTERS: usize = 4;
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

/// The chapters to download ahead while reading chapter `current`.
fn next_chapters(chapters: &[Chapter], current: usize) -> &[Chapter] {
    let start = (current + 1).min(chapters.len());
    &chapters[start..(start + PREFETCH_CHAPTERS).min(chapters.len())]
}

/// Part of the pending scroll to do after `dt`: by elapsed time, so the glide is the same at any
/// frame rate. At least 1 px, never past `pending`.
fn ease(pending: i64, dt: Duration) -> i64 {
    let share = 1.0 - (-dt.as_secs_f64() / GLIDE.as_secs_f64()).exp();
    match (pending as f64 * share).round() as i64 {
        0 => pending.signum(),
        step => step,
    }
}

enum Page {
    Loading,
    Failed(String),
    Ready(Stored),
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
    let starts = |c: &Chapter| c.number.to_string().starts_with(filter);
    (0..chapters.len())
        .filter(|&i| starts(&chapters[i]))
        .collect()
}

struct Reader<'a> {
    io: &'a mut Pages,
    title: String,
    chapters: Vec<Chapter>,
    chapter: usize,
    job: u64,
    pages: Vec<Page>,
    pos: Pos,
    /// Pixels still to scroll, done a bit each frame.
    pending: i64,
    /// Scroll actions in this chapter, to start the prefetch.
    scrolls: u32,
    /// When the previous frame was drawn.
    frame_at: Instant,
    geo: Geo,
    /// Pixel heights of each page and of the view, from the last frame.
    heights: Vec<u32>,
    view: u32,
    bar: bool,
    overlay: Option<Overlay>,
    /// What kitty shows now.
    shown: Vec<Placement>,
}

pub fn run(
    terminal: &mut DefaultTerminal,
    io: &mut Pages,
    title: String,
    chapters: Vec<Chapter>,
) -> Result<Exit> {
    let mut reader = Reader {
        io,
        title,
        chapters,
        chapter: 0,
        job: 0,
        pages: Vec::new(),
        pos: Pos::default(),
        pending: 0,
        scrolls: 0,
        frame_at: Instant::now(),
        geo: Geo::now()?,
        heights: Vec::new(),
        view: 0,
        bar: true,
        overlay: Some(chapter_list(0)),
        shown: Vec::new(),
    };
    // Nothing is downloaded until a chapter is chosen in the list.
    let exit = reader.event_loop(terminal);
    reader.delete_images()?;
    exit
}

impl Reader<'_> {
    fn open(&mut self, chapter: usize) -> Result<()> {
        self.chapter = chapter;
        self.pos = Pos::default();
        self.pending = 0;
        self.scrolls = 0;
        self.load()
    }

    /// (Re)loads the pages of the current chapter at the current width, from the visible page on.
    fn load(&mut self) -> Result<()> {
        self.delete_images()?;
        let chapter = self.chapters[self.chapter];
        self.pages = (0..chapter.pages).map(|_| Page::Loading).collect();
        let start = (self.pos.page as u32 + 1).min(chapter.pages);
        self.job = self
            .io
            .request(&self.title, chapter, start, self.geo.page_px());
        Ok(())
    }

    /// Frees the chapter's images from kitty's memory.
    fn delete_images(&mut self) -> Result<()> {
        self.shown.clear();
        kitty::free(self.pages.iter().filter_map(|page| match page {
            Page::Ready(image) => Some(image.id),
            _ => None,
        }))
    }

    fn receive(&mut self, loaded: Loaded) -> Result<()> {
        if loaded.job != self.job {
            return Ok(());
        }
        let Some(slot) = self.pages.get_mut(loaded.page as usize - 1) else {
            return Ok(());
        };
        *slot = match loaded.image {
            Ok(image) => Page::Ready(kitty::transmit(&image)?),
            Err(e) => Page::Failed(e),
        };
        Ok(())
    }

    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<Exit> {
        loop {
            while let Ok(loaded) = self.io.done.try_recv() {
                self.receive(loaded)?;
            }
            let geo = Geo::now()?;
            if geo != self.geo {
                // Resizing clears the screen, which drops kitty's images: send them again at the new width.
                self.geo = geo;
                if !self.pages.is_empty() {
                    self.load()?;
                }
            }
            let now = Instant::now();
            // Capped, so that the time spent waiting for a key doesn't turn into a jump.
            self.animate(
                now.duration_since(self.frame_at)
                    .min(Duration::from_millis(16)),
            );
            self.frame_at = now;
            let mut placements = Vec::new();
            // Kitty shows the text and the images of a frame together.
            execute!(stdout(), BeginSynchronizedUpdate)?;
            terminal.draw(|frame| placements = self.draw(frame))?;
            kitty::update(&mut self.shown, placements)?;
            execute!(stdout(), EndSynchronizedUpdate)?;
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
        let pos = scroll(&self.heights, self.view, self.geo.cell_h, self.pos, step);
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
                    .map(|i| tf("reader.chapter", &[("n", &self.chapters[i].number)]))
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
        self.pos = scroll(&self.heights, self.view, self.geo.cell_h, self.pos, 0);
    }

    /// Draws "Chargement…", errors and the end line; returns where the page images go.
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
                Some(_) => tf("reader.end", &[("n", &self.chapters[self.chapter].number)]),
                None => t("reader.last").to_string(),
            };
            let line = Rect::new(main.x, main.y + end_row, main.width, 1);
            frame.render_widget(Paragraph::new(end).centered().dim(), line);
        }
        placements
    }

    fn draw_bar(&self, frame: &mut Frame, area: Rect) {
        let chapter = self.chapters[self.chapter];
        let status = tf(
            "reader.status",
            &[
                ("title", &self.title.trim()),
                ("chapter", &chapter.number),
                ("chapters", &self.chapters.len()),
                ("page", &(self.pos.page + 1)),
                ("pages", &chapter.pages),
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
                    self.load()?;
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
            let next = next_chapters(&self.chapters, self.chapter);
            self.io.prefetch(&self.title, next);
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

    #[test]
    fn prefetched_chapters() {
        let chapters: Vec<_> = (1..=6).map(|number| Chapter { number, pages: 1 }).collect();
        let numbers = |current| {
            let next = next_chapters(&chapters, current);
            next.iter().map(|c| c.number).collect::<Vec<_>>()
        };
        assert_eq!(numbers(0), [2, 3, 4, 5]);
        assert_eq!(numbers(4), [6]);
        assert!(numbers(5).is_empty());
    }

    #[test]
    fn chapter_filter() {
        let chapters = [1, 2, 12, 21, 120].map(|number| Chapter { number, pages: 1 });
        assert_eq!(matching(&chapters, ""), [0, 1, 2, 3, 4]);
        assert_eq!(matching(&chapters, "12"), [2, 4]);
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
