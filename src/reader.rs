use std::fmt::Write as _;
use std::io::{Write, stdout};
use std::os::unix::ffi::OsStrExt;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use ratatui::crossterm::terminal::window_size;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::widgets::{ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::Result;
use crate::api::Chapter;
use crate::pages::{Image, Loaded, Pages};

/// Pages are at most this wide, like on the site.
const MAX_WIDTH_PX: u32 = 900;
/// Rows scrolled by j/k and by one mouse wheel notch.
const STEP_ROWS: u32 = 3;
/// After this many scrolls in a chapter, the next chapters are downloaded ahead.
const PREFETCH_AFTER: u32 = 3;
const PREFETCH_CHAPTERS: usize = 4;
/// Share of the remaining scroll done each frame: an ease-out, like CSS `scroll-behavior: smooth`.
const EASE: f64 = 0.3;

const HELP: [&str; 8] = [
    "j k  molette  défiler",
    "d u           demi-écran",
    "h l           chapitre précédent / suivant",
    "F1            liste des chapitres",
    "F2            masquer la barre",
    "?             cette aide",
    "Échap         retour",
    "q             quitter",
];

#[derive(PartialEq)]
pub enum Exit {
    Quit,
    Back,
}

fn kitty(commands: &str) -> Result<()> {
    let mut out = stdout().lock();
    out.write_all(commands.as_bytes())?;
    out.flush()?;
    Ok(())
}

/// Hands the pixels to kitty through a temporary file that kitty deletes after reading (`t=t`),
/// so megabytes of image never go through the terminal.
fn transmit(id: u32, image: &Image) -> Result<()> {
    let name = format!(
        "tty-graphics-protocol-manga-sama-{}-{id}",
        std::process::id()
    );
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, &image.rgb)?;
    let path = STANDARD.encode(path.as_os_str().as_bytes());
    kitty(&format!(
        "\x1b_Ga=t,t=t,f=24,s={},v={},i={id},q=2;{path}\x1b\\",
        image.width, image.height
    ))
}

#[derive(Clone, Copy, PartialEq)]
struct Geo {
    cols: u16,
    rows: u16,
    cell_w: u32,
    cell_h: u32,
}

impl Geo {
    fn now() -> Result<Self> {
        let size = window_size()?;
        if size.columns == 0
            || size.rows == 0
            || size.width < size.columns
            || size.height < size.rows
        {
            return Err(
                "le terminal ne donne pas sa taille en pixels : lance manga-sama dans kitty".into(),
            );
        }
        Ok(Geo {
            cols: size.columns,
            rows: size.rows,
            cell_w: u32::from(size.width / size.columns),
            cell_h: u32::from(size.height / size.rows),
        })
    }

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

/// Part of the pending scroll to do this frame: at least 1 px, never past it.
fn ease(pending: i64) -> i64 {
    match (pending as f64 * EASE) as i64 {
        0 => pending.signum(),
        step => step,
    }
}

/// Part of an image shown 1:1: source rectangle in pixels, drawn from cell (`x`, `y`) + `offset` px down.
#[derive(PartialEq)]
struct Placement {
    id: u32,
    x: u16,
    y: u16,
    offset: u32,
    src_y: u32,
    src_w: u32,
    src_h: u32,
}

enum Page {
    Loading,
    Failed(String),
    Ready { id: u32, width: u32, height: u32 },
}

enum Overlay {
    Chapters(ListState),
    Help,
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
    geo: Geo,
    /// Pixel heights of each page and of the view, from the last frame.
    heights: Vec<u32>,
    view: u32,
    bar: bool,
    overlay: Option<Overlay>,
    last_id: u32,
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
        geo: Geo::now()?,
        heights: Vec::new(),
        view: 0,
        bar: true,
        overlay: Some(Overlay::Chapters(
            ListState::default().with_selected(Some(0)),
        )),
        last_id: 0,
        shown: Vec::new(),
    };
    reader.load()?;
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

    fn delete_images(&mut self) -> Result<()> {
        let mut commands = String::new();
        for page in &self.pages {
            if let Page::Ready { id, .. } = page {
                write!(commands, "\x1b_Ga=d,d=I,i={id},q=2\x1b\\")?;
            }
        }
        self.shown.clear();
        kitty(&commands)
    }

    fn receive(&mut self, loaded: Loaded) -> Result<()> {
        if loaded.job != self.job {
            return Ok(());
        }
        let Some(slot) = self.pages.get_mut(loaded.page as usize - 1) else {
            return Ok(());
        };
        *slot = match loaded.image {
            Ok(image) => {
                self.last_id += 1;
                transmit(self.last_id, &image)?;
                Page::Ready {
                    id: self.last_id,
                    width: image.width,
                    height: image.height,
                }
            }
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
                self.load()?;
            }
            self.animate();
            let mut placements = Vec::new();
            terminal.draw(|frame| placements = self.draw(frame))?;
            self.place(placements)?;
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

    fn animate(&mut self) {
        if self.pending == 0 {
            return;
        }
        let step = ease(self.pending);
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
        let geo = self.geo;
        let placeholder = geo.page_px() * 3 / 2;
        self.heights = self
            .pages
            .iter()
            .map(|page| match page {
                Page::Ready { height, .. } => *height,
                _ => placeholder,
            })
            .collect();
        self.view = u32::from(main.height) * geo.cell_h;
        self.pos = scroll(&self.heights, self.view, geo.cell_h, self.pos, 0);

        let cols = geo.page_cols().min(main.width);
        let x = main.x + (main.width - cols) / 2;
        // Pixels from the top of `main`.
        let mut y = 0;
        let mut placements = Vec::new();
        for (page, top, height) in visible(&self.heights, self.view, self.pos) {
            let row = (y / geo.cell_h) as u16;
            match &self.pages[page] {
                Page::Ready { id, width, .. } => placements.push(Placement {
                    id: *id,
                    x,
                    y: main.y + row,
                    offset: y % geo.cell_h,
                    src_y: top,
                    src_w: *width,
                    src_h: height,
                }),
                other => {
                    let rows = (y + height).div_ceil(geo.cell_h) as u16 - row;
                    let area = Rect::new(x, main.y + row, cols, rows);
                    match other {
                        Page::Failed(e) => message(frame, area, &format!("Erreur : {e}")),
                        _ => message(frame, area, "Chargement…"),
                    }
                }
            }
            y += height;
        }
        let end_row = y.div_ceil(geo.cell_h) as u16;
        if end_row < main.height {
            let end = match self.chapters.get(self.chapter + 1) {
                Some(_) => format!(
                    "Fin du chapitre {} · l : chapitre suivant",
                    self.chapters[self.chapter].number
                ),
                None => "Dernier chapitre.".to_string(),
            };
            let line = Rect::new(main.x, main.y + end_row, main.width, 1);
            frame.render_widget(Paragraph::new(end).centered().dim(), line);
        }

        if self.bar {
            let chapter = self.chapters[self.chapter];
            let status = format!(
                "{} · Chapitre {}/{} · Page {}/{} · ? aide",
                self.title.trim(),
                chapter.number,
                self.chapters.len(),
                self.pos.page + 1,
                chapter.pages,
            );
            frame.render_widget(Paragraph::new(status).reversed(), bar);
        }

        match &mut self.overlay {
            None => return placements,
            Some(Overlay::Chapters(list)) => {
                let items = self
                    .chapters
                    .iter()
                    .map(|c| format!("Chapitre {}", c.number));
                crate::popup(frame, "Chapitres", items.collect(), list);
            }
            Some(Overlay::Help) => {
                let items = HELP.iter().map(|line| line.to_string()).collect();
                crate::popup(frame, "Aide", items, &mut ListState::default());
            }
        }
        // Images are drawn above the text: hide them while a popup is open.
        Vec::new()
    }

    /// Sends kitty only what changed since the last frame, in one write.
    fn place(&mut self, placements: Vec<Placement>) -> Result<()> {
        if placements == self.shown {
            return Ok(());
        }
        let mut commands = String::new();
        for gone in self
            .shown
            .iter()
            .filter(|old| !placements.iter().any(|p| p.id == old.id))
        {
            write!(commands, "\x1b_Ga=d,d=i,i={},q=2\x1b\\", gone.id)?;
        }
        // Same image id and placement id (p=1): kitty moves the placement instead of adding one.
        for p in placements.iter().filter(|p| !self.shown.contains(p)) {
            write!(
                commands,
                "\x1b[{};{}H\x1b_Ga=p,i={},p=1,x=0,y={},w={},h={},Y={},C=1,q=2\x1b\\",
                p.y + 1,
                p.x + 1,
                p.id,
                p.src_y,
                p.src_w,
                p.src_h,
                p.offset,
            )?;
        }
        kitty(&commands)?;
        self.shown = placements;
        Ok(())
    }

    fn handle(&mut self, event: Event) -> Result<Option<Exit>> {
        let code = match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(Some(Exit::Quit));
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
            KeyCode::F(2) => self.bar = !self.bar,
            _ if self.overlay.is_some() => self.handle_overlay(code)?,
            KeyCode::Esc => return Ok(Some(Exit::Back)),
            KeyCode::F(1) => {
                let list = ListState::default().with_selected(Some(self.chapter));
                self.overlay = Some(Overlay::Chapters(list));
            }
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
        let Some(Overlay::Chapters(list)) = &mut self.overlay else {
            // Any key closes the help.
            self.overlay = None;
            return Ok(());
        };
        match code {
            KeyCode::Char('j') | KeyCode::Down => list.select_next(),
            KeyCode::Char('k') | KeyCode::Up => list.select_previous(),
            KeyCode::Enter => {
                // ListState only clamps its selection when rendered.
                let chapter = list.selected().unwrap_or(0).min(self.chapters.len() - 1);
                self.overlay = None;
                self.open(chapter)?;
            }
            KeyCode::F(1) | KeyCode::Esc => self.overlay = None,
            _ => {}
        }
        Ok(())
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
        assert_eq!(ease(60), 18);
        assert_eq!(ease(-60), -18);
        assert_eq!(ease(3), 1);
        assert_eq!(ease(-1), -1);
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
