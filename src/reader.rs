use std::io::stdout;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Stylize};
use ratatui::widgets::{Block, Clear, List, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{FilterType, Resize, StatefulImage};

use crate::Result;
use crate::api::{self, Chapter};

/// Chapter number, page count, first page to fetch.
type Job = (u32, u32, u32);
/// Chapter number, page, image bytes.
type Loaded = (u32, u32, Result<Vec<u8>, String>);

/// Indexes into the chapter list and the chapter's pages, from 0.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pos {
    chapter: usize,
    page: usize,
}

enum Move {
    NextPage,
    PrevPage,
    NextChapter,
    PrevChapter,
}

fn step(chapters: &[Chapter], pos: Pos, mv: Move) -> Pos {
    let last_page = |chapter: usize| (chapters[chapter].pages as usize).saturating_sub(1);
    let last_chapter = chapters.len().saturating_sub(1);
    match mv {
        Move::NextPage if pos.page < last_page(pos.chapter) => Pos {
            page: pos.page + 1,
            ..pos
        },
        Move::PrevPage if pos.page > 0 => Pos {
            page: pos.page - 1,
            ..pos
        },
        Move::NextPage | Move::NextChapter if pos.chapter < last_chapter => Pos {
            chapter: pos.chapter + 1,
            page: 0,
        },
        Move::PrevPage if pos.chapter > 0 => Pos {
            chapter: pos.chapter - 1,
            page: last_page(pos.chapter - 1),
        },
        Move::PrevChapter if pos.chapter > 0 => Pos {
            chapter: pos.chapter - 1,
            page: 0,
        },
        _ => pos,
    }
}

struct App {
    title: String,
    chapters: Vec<Chapter>,
    pos: Pos,
    /// Pages of the current chapter only, `None` until downloaded.
    pages: Vec<Option<Result<Vec<u8>, String>>>,
    shown: Option<(Pos, Result<StatefulProtocol, String>)>,
    bar: bool,
    /// Chapter list, when open.
    list: Option<ListState>,
    jobs: Sender<Job>,
}

impl App {
    fn go(&mut self, pos: Pos) {
        let new_chapter = pos.chapter != self.pos.chapter;
        self.pos = pos;
        if new_chapter {
            self.load_chapter();
        }
    }

    fn load_chapter(&mut self) {
        let chapter = self.chapters[self.pos.chapter];
        self.pages = vec![None; chapter.pages as usize];
        self.shown = None;
        // Fails only if the download thread is gone; the page then stays "Chargement…".
        let _ = self
            .jobs
            .send((chapter.number, chapter.pages, self.pos.page as u32 + 1));
    }

    fn open_list(&mut self) {
        self.list = Some(ListState::default().with_selected(Some(self.pos.chapter)));
    }

    /// Returns true to quit.
    fn handle(&mut self, event: Event) -> bool {
        let code = match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => key.code,
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::ScrollDown => KeyCode::Down,
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::ScrollUp => KeyCode::Up,
            _ => return false,
        };
        match code {
            KeyCode::Char('q') => return true,
            KeyCode::F(2) => self.bar = !self.bar,
            _ if self.list.is_some() => self.handle_list(code),
            KeyCode::Esc => return true,
            KeyCode::F(1) => self.open_list(),
            KeyCode::Char('j' | ' ') | KeyCode::Down => {
                self.go(step(&self.chapters, self.pos, Move::NextPage))
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.go(step(&self.chapters, self.pos, Move::PrevPage))
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.go(step(&self.chapters, self.pos, Move::NextChapter))
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.go(step(&self.chapters, self.pos, Move::PrevChapter))
            }
            _ => {}
        }
        false
    }

    fn handle_list(&mut self, code: KeyCode) {
        let Some(list) = &mut self.list else { return };
        match code {
            KeyCode::Char('j') | KeyCode::Down => list.select_next(),
            KeyCode::Char('k') | KeyCode::Up => list.select_previous(),
            KeyCode::Enter => {
                // ListState only clamps its selection when rendered.
                let chapter = list.selected().unwrap_or(0).min(self.chapters.len() - 1);
                self.list = None;
                self.go(Pos { chapter, page: 0 });
            }
            KeyCode::F(1) | KeyCode::Esc => self.list = None,
            _ => {}
        }
    }

    fn receive(&mut self, done: &Receiver<Loaded>) {
        let current = self.chapters[self.pos.chapter].number;
        for (chapter, page, result) in done.try_iter() {
            if chapter == current
                && let Some(slot) = self.pages.get_mut(page as usize - 1)
            {
                *slot = Some(result);
            }
        }
    }

    // ponytail: decoding here and resizing at render block the UI briefly on each page change
    // (a fraction of a second in release); move them to ratatui_image::thread::ThreadProtocol if it lags.
    fn decode_current(&mut self, picker: &Picker) {
        if self.shown.as_ref().is_some_and(|(pos, _)| *pos == self.pos) {
            return;
        }
        let Some(Some(page)) = self.pages.get(self.pos.page) else {
            return;
        };
        let image = match page {
            Ok(bytes) => image::load_from_memory(bytes)
                .map(|image| picker.new_resize_protocol(image))
                .map_err(|e| format!("image illisible : {e}")),
            Err(e) => Err(e.clone()),
        };
        self.shown = Some((self.pos, image));
    }
}

fn draw(frame: &mut Frame, app: &mut App) {
    let [main, bar] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(u16::from(app.bar))])
            .areas(frame.area());

    match &mut app.shown {
        Some((pos, Ok(image))) if *pos == app.pos => {
            let resize = Resize::Scale(Some(FilterType::Triangle));
            let size = image.size_for(resize.clone(), main.as_size());
            let area = main.centered(
                Constraint::Length(size.width),
                Constraint::Length(size.height),
            );
            frame.render_stateful_widget(StatefulImage::default().resize(resize), area, image);
        }
        Some((pos, Err(e))) if *pos == app.pos => message(frame, main, &format!("Erreur : {e}")),
        _ => message(frame, main, "Chargement…"),
    }

    if app.bar {
        let chapter = app.chapters[app.pos.chapter];
        let status = format!(
            "{} · Chapitre {}/{} · Page {}/{} · F1 chapitres · F2 masquer · q quitter",
            app.title.trim(),
            chapter.number,
            app.chapters.len(),
            app.pos.page + 1,
            chapter.pages,
        );
        frame.render_widget(Paragraph::new(status).reversed(), bar);
    }

    if let Some(list) = &mut app.list {
        let area = main.centered(Constraint::Length(24), Constraint::Percentage(80));
        let items = app
            .chapters
            .iter()
            .map(|c| format!("Chapitre {}", c.number));
        let list_widget = List::new(items)
            .block(Block::bordered().title(" Chapitres "))
            .highlight_style(Modifier::REVERSED);
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(list_widget, area, list);
    }
}

fn message(frame: &mut Frame, area: Rect, text: &str) {
    let text = Paragraph::new(text).centered().wrap(Wrap { trim: true });
    frame.render_widget(text, area.centered_vertically(Constraint::Length(3)));
}

/// Fetches the pages of the requested chapter one at a time, starting at the requested page.
/// A new job replaces the current one between two pages.
fn download(title: &str, jobs: Receiver<Job>, done: Sender<Loaded>) {
    let mut queue: Vec<(u32, u32)> = Vec::new();
    loop {
        let job = if queue.is_empty() {
            match jobs.recv() {
                Ok(job) => Some(job),
                Err(_) => return,
            }
        } else {
            jobs.try_iter().last()
        };
        if let Some((chapter, count, start)) = job {
            // Popped from the end: `start` first, the rest of the chapter, then the pages before it.
            queue = (start..=count)
                .chain(1..start)
                .rev()
                .map(|page| (chapter, page))
                .collect();
        }
        let Some((chapter, page)) = queue.pop() else {
            continue;
        };
        let result = api::page(title, chapter, page).map_err(|e| e.to_string());
        if done.send((chapter, page, result)).is_err() {
            return;
        }
    }
}

pub fn run(title: String, chapters: Vec<Chapter>) -> Result<()> {
    let (jobs, job_rx) = mpsc::channel();
    let (done_tx, done) = mpsc::channel();
    let api_title = title.clone();
    thread::spawn(move || download(&api_title, job_rx, done_tx));

    let mut app = App {
        title,
        chapters,
        pos: Pos {
            chapter: 0,
            page: 0,
        },
        pages: Vec::new(),
        shown: None,
        bar: true,
        list: None,
        jobs,
    };
    app.load_chapter();
    app.open_list();

    let mut terminal = ratatui::init();
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture);
        hook(info);
    }));
    let result = event_loop(&mut terminal, &mut app, &done);
    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    done: &Receiver<Loaded>,
) -> Result<()> {
    // Must run before mouse capture and event reading: it queries the terminal on stdin.
    let picker = Picker::from_query_stdio()?;
    execute!(stdout(), EnableMouseCapture)?;
    loop {
        app.receive(done);
        app.decode_current(&picker);
        terminal.draw(|frame| draw(frame, app))?;
        // Drain the whole burst (mouse wheel) so that only the final page gets decoded.
        let mut timeout = Duration::from_millis(50);
        while event::poll(timeout)? {
            if app.handle(event::read()?) {
                return Ok(());
            }
            timeout = Duration::ZERO;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAPTERS: [Chapter; 2] = [
        Chapter {
            number: 1,
            pages: 3,
        },
        Chapter {
            number: 2,
            pages: 2,
        },
    ];

    fn at(chapter: usize, page: usize) -> Pos {
        Pos { chapter, page }
    }

    #[test]
    fn pages() {
        assert_eq!(step(&CHAPTERS, at(0, 0), Move::NextPage), at(0, 1));
        assert_eq!(step(&CHAPTERS, at(0, 2), Move::NextPage), at(1, 0));
        assert_eq!(step(&CHAPTERS, at(1, 1), Move::NextPage), at(1, 1));
        assert_eq!(step(&CHAPTERS, at(0, 1), Move::PrevPage), at(0, 0));
        assert_eq!(step(&CHAPTERS, at(1, 0), Move::PrevPage), at(0, 2));
        assert_eq!(step(&CHAPTERS, at(0, 0), Move::PrevPage), at(0, 0));
    }

    #[test]
    fn chapters() {
        assert_eq!(step(&CHAPTERS, at(0, 2), Move::NextChapter), at(1, 0));
        assert_eq!(step(&CHAPTERS, at(1, 1), Move::NextChapter), at(1, 1));
        assert_eq!(step(&CHAPTERS, at(1, 1), Move::PrevChapter), at(0, 0));
        assert_eq!(step(&CHAPTERS, at(0, 1), Move::PrevChapter), at(0, 1));
    }
}
