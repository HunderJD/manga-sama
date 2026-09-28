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
const STEP: i64 = 3;

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

    /// Rows taken by a `width`×`height` image drawn `page_cols` wide.
    fn rows_for(self, width: u32, height: u32) -> u32 {
        let px = u64::from(self.page_cols()) * u64::from(self.cell_w) * u64::from(height)
            / u64::from(width.max(1));
        let cell = u64::from(self.cell_h);
        (((px + cell / 2) / cell) as u32).max(1)
    }
}

/// Top of the view: a page and a row inside it, so pages loading above don't move what is read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pos {
    page: usize,
    row: u32,
}

/// Moves the view by `delta` rows, never above the first page nor below the end line.
fn scroll(heights: &[u32], view: u32, pos: Pos, delta: i64) -> Pos {
    let above: u32 = heights[..pos.page.min(heights.len())].iter().sum();
    // +1 for the end-of-chapter line.
    let max = (heights.iter().sum::<u32>() + 1).saturating_sub(view);
    let mut offset = (i64::from(above + pos.row) + delta).clamp(0, i64::from(max)) as u32;
    let mut page = 0;
    while page + 1 < heights.len() && offset >= heights[page] {
        offset -= heights[page];
        page += 1;
    }
    Pos { page, row: offset }
}

/// The slices of pages the view shows from `pos`, top to bottom: (page, first row, rows).
fn visible(heights: &[u32], view: u32, pos: Pos) -> Vec<(usize, u32, u32)> {
    let mut slices = Vec::new();
    let (mut page, mut row, mut left) = (pos.page, pos.row, view);
    while left > 0 && page < heights.len() {
        let rows = heights[page].saturating_sub(row).min(left);
        if rows > 0 {
            slices.push((page, row, rows));
        }
        left -= rows;
        page += 1;
        row = 0;
    }
    slices
}

/// Part of an image shown on screen: source rectangle in pixels, target in cells.
#[derive(PartialEq)]
struct Placement {
    id: u32,
    x: u16,
    y: u16,
    cols: u16,
    rows: u16,
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
    geo: Geo,
    /// Rows of each page and of the view, from the last frame.
    heights: Vec<u32>,
    view: u32,
    bar: bool,
    overlay: Option<Overlay>,
    last_id: u32,
    /// What kitty shows now, to skip identical updates.
    shown: (Geo, Vec<Placement>),
}

pub fn run(
    terminal: &mut DefaultTerminal,
    io: &mut Pages,
    title: String,
    chapters: Vec<Chapter>,
) -> Result<Exit> {
    let geo = Geo::now()?;
    let mut reader = Reader {
        io,
        title,
        chapters,
        chapter: 0,
        job: 0,
        pages: Vec::new(),
        pos: Pos::default(),
        geo,
        heights: Vec::new(),
        view: 0,
        bar: true,
        overlay: Some(Overlay::Chapters(
            ListState::default().with_selected(Some(0)),
        )),
        last_id: 0,
        shown: (geo, Vec::new()),
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
        self.load()
    }

    /// (Re)loads the pages of the current chapter at the current width, from the visible page on.
    fn load(&mut self) -> Result<()> {
        self.delete_images()?;
        let chapter = self.chapters[self.chapter];
        self.pages = (0..chapter.pages).map(|_| Page::Loading).collect();
        let width = u32::from(self.geo.page_cols()) * self.geo.cell_w;
        let start = (self.pos.page as u32 + 1).min(chapter.pages);
        self.job = self.io.request(&self.title, chapter, start, width);
        Ok(())
    }

    fn delete_images(&mut self) -> Result<()> {
        let mut commands = String::new();
        for page in &self.pages {
            if let Page::Ready { id, .. } = page {
                write!(commands, "\x1b_Ga=d,d=I,i={id},q=2\x1b\\")?;
            }
        }
        self.shown.1.clear();
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
                // Resizing clears the screen, which can drop kitty's images: send them again at the new width.
                self.geo = geo;
                self.load()?;
            }
            let mut placements = Vec::new();
            terminal.draw(|frame| placements = self.draw(frame))?;
            self.place(placements)?;
            // Drain the whole burst (mouse wheel) before drawing again.
            let mut timeout = Duration::from_millis(50);
            while event::poll(timeout)? {
                if let Some(exit) = self.handle(event::read()?)? {
                    return Ok(exit);
                }
                timeout = Duration::ZERO;
            }
        }
    }

    /// Draws the text and returns where kitty must draw the images.
    fn draw(&mut self, frame: &mut Frame) -> Vec<Placement> {
        let [main, bar] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(u16::from(self.bar))])
                .areas(frame.area());
        let geo = self.geo;
        self.heights = self
            .pages
            .iter()
            .map(|page| match page {
                Page::Ready { width, height, .. } => geo.rows_for(*width, *height),
                _ => geo.rows_for(2, 3),
            })
            .collect();
        self.view = main.height.into();
        self.pos = scroll(&self.heights, self.view, self.pos, 0);

        let cols = geo.page_cols().min(main.width);
        let x = main.x + (main.width - cols) / 2;
        let mut y = main.y;
        let mut placements = Vec::new();
        for (page, row, rows) in visible(&self.heights, self.view, self.pos) {
            let area = Rect::new(x, y, cols, rows as u16);
            match &self.pages[page] {
                Page::Ready { id, width, height } => {
                    let to_px = |r: u32| r * height / self.heights[page];
                    placements.push(Placement {
                        id: *id,
                        x,
                        y,
                        cols,
                        rows: rows as u16,
                        src_y: to_px(row),
                        src_w: *width,
                        src_h: to_px(rows).max(1),
                    });
                }
                Page::Loading => message(frame, area, "Chargement…"),
                Page::Failed(e) => message(frame, area, &format!("Erreur : {e}")),
            }
            y += rows as u16;
        }
        if y < main.bottom() {
            let end = match self.chapters.get(self.chapter + 1) {
                Some(_) => format!(
                    "Fin du chapitre {} · l : chapitre suivant",
                    self.chapters[self.chapter].number
                ),
                None => "Dernier chapitre.".to_string(),
            };
            let line = Rect::new(main.x, y, main.width, 1);
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

    /// Updates kitty in one write (delete everything, place again), so it never shows a half-updated frame.
    fn place(&mut self, placements: Vec<Placement>) -> Result<()> {
        let now = (self.geo, placements);
        if now == self.shown {
            return Ok(());
        }
        let mut commands = String::from("\x1b_Ga=d,d=a,q=2\x1b\\");
        for p in &now.1 {
            write!(
                commands,
                "\x1b[{};{}H\x1b_Ga=p,i={},p=1,x=0,y={},w={},h={},c={},r={},C=1,q=2\x1b\\",
                p.y + 1,
                p.x + 1,
                p.id,
                p.src_y,
                p.src_w,
                p.src_h,
                p.cols,
                p.rows,
            )?;
        }
        kitty(&commands)?;
        self.shown = now;
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
            KeyCode::Char('j') | KeyCode::Down => self.scroll_by(STEP),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_by(-STEP),
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
        self.pos = scroll(&self.heights, self.view, self.pos, delta);
    }
}

fn message(frame: &mut Frame, area: Rect, text: &str) {
    let text = Paragraph::new(text).centered().wrap(Wrap { trim: true });
    frame.render_widget(text, area.centered_vertically(Constraint::Length(3)));
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEIGHTS: [u32; 3] = [10, 20, 5];

    fn at(page: usize, row: u32) -> Pos {
        Pos { page, row }
    }

    #[test]
    fn scrolling() {
        assert_eq!(scroll(&HEIGHTS, 8, at(0, 0), 3), at(0, 3));
        assert_eq!(scroll(&HEIGHTS, 8, at(0, 9), 3), at(1, 2));
        assert_eq!(scroll(&HEIGHTS, 8, at(1, 2), -3), at(0, 9));
        assert_eq!(scroll(&HEIGHTS, 8, at(0, 1), -5), at(0, 0));
        // 35 rows + the end line, in a view of 8: the bottom is 28 rows down.
        assert_eq!(scroll(&HEIGHTS, 8, at(2, 0), 100), at(1, 18));
        assert_eq!(scroll(&[3], 8, at(0, 0), 5), at(0, 0));
    }

    #[test]
    fn visible_slices() {
        assert_eq!(visible(&HEIGHTS, 8, at(0, 7)), [(0, 7, 3), (1, 0, 5)]);
        assert_eq!(visible(&HEIGHTS, 8, at(1, 18)), [(1, 18, 2), (2, 0, 5)]);
    }

    #[test]
    fn page_rows() {
        let geo = Geo {
            cols: 200,
            rows: 50,
            cell_w: 10,
            cell_h: 20,
        };
        assert_eq!(geo.page_cols(), 90);
        // 1920×3005 shown 900 px wide: 1408 px, 70.4 rows.
        assert_eq!(geo.rows_for(1920, 3005), 70);
    }
}
