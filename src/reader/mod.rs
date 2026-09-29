//! The reading screen: what you see, keys, scrolling, and asking the page loader for pages.

pub mod page_loader;
mod scroll;

use std::ops::Range;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

use self::page_loader::{Loaded, PageLoader, Plan};
use self::scroll::{AHEAD, Pos, ease, near_end, to_download, to_free, visible, window};
use crate::Result;
use crate::i18n::{t, tf};
use crate::kitty::{self, Geo, Placement, Stored};
use crate::sources::anime_sama::Chapter;
use crate::ui::{Popups, Shared};

/// How the reader is left.
enum Exit {
    /// To the chapter list (Backspace, Esc, F1).
    Chapters,
    Quit,
}

const HELP: &[(&str, &str)] = &[
    ("j k", "help.scroll"),
    ("d u", "help.half"),
    ("h l", "help.chapter"),
    ("F1", "help.chapters"),
    ("Shift+H", "help.bar"),
    ("Backspace Esc", "help.back"),
    ("q", "help.quit"),
];

/// Rows scrolled by j/k and by one mouse wheel notch.
const STEP_ROWS: u32 = 3;

enum Page {
    /// Not in kitty: not decoded yet, or freed since. A known height is kept so the layout
    /// doesn't move.
    Waiting(Option<u32>),
    Ready(Stored),
    Failed(String),
}

impl Page {
    fn height(&self) -> Option<u32> {
        match self {
            Page::Waiting(height) => *height,
            Page::Ready(image) => Some(image.height),
            Page::Failed(_) => None,
        }
    }
}

struct Reader<'a> {
    loader: &'a mut PageLoader,
    title: &'a str,
    chapters: &'a [Chapter],
    /// Index of the chapter read, in `chapters`.
    current: usize,
    job: u64,
    pages: Vec<Page>,
    pos: Pos,
    /// Pixels still to scroll, done a bit each frame.
    pending: i64,
    /// What the last plan was made for (pages to decode, pages to download, next chapter's
    /// start): a new plan is sent only when it changes.
    planned: Option<(Range<usize>, Range<usize>, bool)>,
    frame_at: Instant,
    geo: Geo,
    /// Pixel heights of each page and of the view, from the last frame.
    heights: Vec<u32>,
    view: u32,
    bar: bool,
    popups: Popups,
}

/// Reads `chapters[chapter]` of `title` (the name the site knows the work by). Returns the chapter
/// being read when going back to the chapter list, `None` to quit.
pub fn run(
    terminal: &mut DefaultTerminal,
    loader: &mut PageLoader,
    title: &str,
    chapters: &[Chapter],
    chapter: usize,
) -> Result<Option<usize>> {
    let mut reader = Reader {
        loader,
        title,
        chapters,
        current: chapter,
        job: 0,
        pages: Vec::new(),
        pos: Pos::default(),
        pending: 0,
        planned: None,
        frame_at: Instant::now(),
        geo: Geo::now()?,
        heights: Vec::new(),
        view: 0,
        bar: true,
        popups: Popups::default(),
    };
    reader.load()?;
    let exit = reader.event_loop(terminal);
    // An empty plan: leaving the reader stops the downloads.
    reader.loader.plan(Plan::default());
    reader.free_images()?;
    Ok(match exit? {
        Exit::Chapters => Some(reader.current),
        Exit::Quit => None,
    })
}

impl Reader<'_> {
    fn open(&mut self, chapter: usize) -> Result<()> {
        self.current = chapter;
        self.pos = Pos::default();
        self.pending = 0;
        self.load()
    }

    /// (Re)loads the pages of the current chapter at the current width, from the visible page on.
    fn load(&mut self) -> Result<()> {
        self.free_images()?;
        let chapter = self.chapters[self.current];
        self.pages = (0..chapter.pages).map(|_| Page::Waiting(None)).collect();
        // Heights change with the width: stay on the same page, but from its top.
        self.pos.px = 0;
        // The old chapter's heights would ask for the wrong pages until the next frame.
        self.heights.clear();
        self.planned = None;
        self.job = self.loader.new_job();
        Ok(())
    }

    fn free_images(&mut self) -> Result<()> {
        kitty::free(self.pages.iter().filter_map(|page| match page {
            Page::Ready(image) => Some(image.id),
            _ => None,
        }))
    }

    fn receive(&mut self, loaded: Loaded) -> Result<()> {
        if loaded.job != self.job {
            return Ok(());
        }
        // A page that already has its image (decoded twice across two plans) keeps it.
        let Some(page @ Page::Waiting(_)) = self.pages.get_mut(loaded.page as usize - 1) else {
            return Ok(());
        };
        *page = match loaded.image {
            Ok(image) => Page::Ready(kitty::transmit(&image)?),
            Err(e) => Page::Failed(e),
        };
        Ok(())
    }

    /// Follows the view: when it moves, a new plan tells the loader what to get, and images
    /// farthest from it are freed once kitty holds more than the budget.
    /// Only the view ± 2 is decoded (decoded pages take kitty's memory), but 5 more pages are
    /// downloaded ahead: the network is the slow part, so it has to start early.
    fn follow_view(&mut self) -> Result<()> {
        let slices = visible(&self.heights, self.view, self.pos);
        let (Some(&(first, ..)), Some(&(last, ..))) = (slices.first(), slices.last()) else {
            return Ok(());
        };
        let len = self.pages.len();
        let near = window(first, last, len);
        let wanted = (
            near.clone(),
            to_download(first, last, len),
            near_end(last, len),
        );
        if self.planned.as_ref() != Some(&wanted) {
            self.send_plan(&wanted);
            self.planned = Some(wanted);
        }
        self.free_far(&near)
    }

    /// Like the site, only what the view is about to reach: the pages around it decoded, the
    /// next ones downloaded, and the next chapter's start once the end is near.
    fn send_plan(&self, (near, ahead, next_start): &(Range<usize>, Range<usize>, bool)) {
        let number = self.chapters[self.current].number;
        let page = |index: usize| (number, index as u32 + 1);
        let decode = near
            .clone()
            .filter(|&index| matches!(self.pages[index], Page::Waiting(_)))
            .map(page)
            .collect();
        let mut download: Vec<_> = ahead.clone().map(page).collect();
        if let Some(next) = self.chapters.get(self.current + 1).filter(|_| *next_start) {
            let start = 1..=next.pages.min(AHEAD as u32);
            download.extend(start.map(|page| (next.number, page)));
        }
        self.loader.plan(Plan {
            job: self.job,
            title: self.title.to_string(),
            width: self.geo.page_px(),
            decode,
            download,
        });
    }

    /// Frees the images farthest from `near` while kitty holds more than the budget.
    fn free_far(&mut self, near: &Range<usize>) -> Result<()> {
        let stored: Vec<_> = self
            .pages
            .iter()
            .enumerate()
            .filter_map(|(index, page)| match page {
                Page::Ready(image) => {
                    Some((index, u64::from(image.width) * u64::from(image.height) * 3))
                }
                _ => None,
            })
            .collect();
        let mut far = Vec::new();
        for index in to_free(&stored, near) {
            if let Page::Ready(image) = &self.pages[index] {
                far.push(image.id);
                self.pages[index] = Page::Waiting(Some(image.height));
            }
        }
        kitty::free(far)
    }

    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<Exit> {
        // What kitty shows now.
        let mut shown = Vec::new();
        loop {
            while let Ok(loaded) = self.loader.done.try_recv() {
                self.receive(loaded)?;
            }
            let geo = Geo::now()?;
            if geo != self.geo {
                // Resizing clears the screen, which drops kitty's images: send them again at the new width.
                self.geo = geo;
                self.load()?;
            }
            self.follow_view()?;
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
        self.popups.draw(frame, HELP);
        // Images are drawn above the text: hide them while a popup is open.
        if self.popups.is_open() {
            Vec::new()
        } else {
            placements
        }
    }

    /// Page heights and view size for this frame, keeping the view inside the chapter.
    fn layout(&mut self, main: Rect) {
        // A page not decoded yet takes the height of a typical 2:3 page.
        let placeholder = self.geo.page_px() * 3 / 2;
        self.heights = self
            .pages
            .iter()
            .map(|page| page.height().unwrap_or(placeholder))
            .collect();
        self.view = u32::from(main.height) * self.geo.cell_h;
        // Moves nothing: only brings `pos` back inside the chapter when heights changed
        // (a page decoded, a resize).
        self.pos = self.scrolled(0);
    }

    /// The position `delta` px away, kept inside the chapter. Below the last page comes the
    /// "end of chapter" line, one cell high: that's the `end` given to `scroll`.
    fn scrolled(&self, delta: i64) -> Pos {
        let end_line = self.geo.cell_h;
        scroll::scroll(&self.heights, self.view, end_line, self.pos, delta)
    }

    /// Draws the loading and error messages and the end line; returns where the page images go.
    fn draw_pages(&self, frame: &mut Frame, main: Rect) -> Vec<Placement> {
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
                    // Very long pages are narrower than the column: centered.
                    let indent = geo.page_px().saturating_sub(image.width) / 2;
                    placements.push(Placement {
                        id: image.id,
                        x: x + (indent / geo.cell_w) as u16,
                        y: main.y + row,
                        x_offset: indent % geo.cell_w,
                        y_offset: y % geo.cell_h,
                        src_y: top,
                        src_w: image.width,
                        src_h: height,
                    });
                    None
                }
                Page::Failed(e) => Some(tf("error", &[("e", e)])),
                _ => Some(t("loading").to_string()),
            };
            if let Some(text) = text {
                let rows = (y + height).div_ceil(geo.cell_h) as u16 - row;
                draw_message(frame, Rect::new(x, main.y + row, cols, rows), &text);
            }
            y += height;
        }

        let end_row = y.div_ceil(geo.cell_h) as u16;
        if end_row < main.height {
            let end = match self.chapters.get(self.current + 1) {
                Some(_) => tf("reader.end", &[("n", &self.chapters[self.current].number)]),
                None => t("reader.last").to_string(),
            };
            let line = Rect::new(main.x, main.y + end_row, main.width, 1);
            frame.render_widget(Paragraph::new(end).centered().dim(), line);
        }
        placements
    }

    fn draw_bar(&self, frame: &mut Frame, area: Rect) {
        let chapter = self.chapters[self.current];
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
            Event::Key(key) if key.kind == KeyEventKind::Press => match self.popups.key(key) {
                Shared::Quit => return Ok(Some(Exit::Quit)),
                Shared::Used => return Ok(None),
                Shared::Screen(key) => key.code,
            },
            // The mouse is ignored while a popup is open.
            Event::Mouse(_) if self.popups.is_open() => return Ok(None),
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
            KeyCode::Backspace | KeyCode::Esc | KeyCode::F(1) => return Ok(Some(Exit::Chapters)),
            KeyCode::Char('H') => self.bar = !self.bar,
            KeyCode::Char('j') | KeyCode::Down => self.pending += step,
            KeyCode::Char('k') | KeyCode::Up => self.pending -= step,
            KeyCode::Char('d') => self.pending += half,
            KeyCode::Char('u') => self.pending -= half,
            KeyCode::Char('l') | KeyCode::Right if self.current + 1 < self.chapters.len() => {
                self.open(self.current + 1)?
            }
            KeyCode::Char('h') | KeyCode::Left if self.current > 0 => {
                self.open(self.current - 1)?
            }
            _ => {}
        }
        Ok(None)
    }
}

fn draw_message(frame: &mut Frame, area: Rect, text: &str) {
    let text = Paragraph::new(text).centered().wrap(Wrap { trim: true });
    frame.render_widget(text, area.centered_vertically(Constraint::Length(3)));
}
