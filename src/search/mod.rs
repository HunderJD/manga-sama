//! The search screen: what you see and the keys (live search, list or tiles).

mod cover_loader;
mod results_loader;
mod tiles;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Position};
use ratatui::style::{Modifier, Stylize};
use ratatui::widgets::{Block, List, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use self::results_loader::{Found, ResultsLoader};
use self::tiles::Tiles;
use crate::Result;
use crate::i18n::{t, tf};
use crate::kitty::{self, Geo, Placement};
use crate::sources::anime_sama::Link;
use crate::ui::{Go, Popups, Shared};

/// Time without typing before searching.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// One letter would list the whole catalogue.
const MIN_CHARS: usize = 2;

const HELP: &[(&str, &str)] = &[
    ("↑ ↓ ← →", "help.select"),
    ("Enter", "help.open"),
    ("Ctrl+T", "help.tiles"),
    ("Esc", "help.quit"),
];

pub struct Search {
    query: String,
    typed_at: Instant,
    /// Last query sent to the search thread.
    asked: String,
    /// Results by query, for the whole session.
    cache: HashMap<String, Vec<Link>>,
    /// Query whose results are on screen.
    shown: String,
    list: ListState,
    status: String,
    popups: Popups,
    /// Results as cover tiles instead of a list; Ctrl+T switches.
    grid: bool,
    tiles: Tiles,
    /// Terminal size; `None` when it gives no pixel size, then there are no covers.
    geo: Option<Geo>,
    results: ResultsLoader,
}

impl Search {
    pub fn new() -> Self {
        Search {
            query: String::new(),
            typed_at: Instant::now(),
            asked: String::new(),
            cache: HashMap::new(),
            shown: String::new(),
            list: ListState::default(),
            status: String::new(),
            popups: Popups::default(),
            grid: false,
            tiles: Tiles::new(),
            geo: None,
            results: ResultsLoader::spawn(),
        }
    }

    /// Runs until a work is chosen (`Next`) or the user quits. The query and the results stay
    /// for the next visit.
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<Go<Link>> {
        // What kitty shows now.
        let mut shown = Vec::new();
        loop {
            while let Ok(found) = self.results.found.try_recv() {
                self.receive(found);
            }
            self.ask();
            self.tiles.receive()?;
            let geo = Geo::now().ok();
            if geo != self.geo {
                // A resize clears kitty's images.
                self.geo = geo;
                self.tiles.reset()?;
            }
            kitty::frame(terminal, &mut shown, |frame| self.draw(frame))?;
            let mut timeout = Duration::from_millis(50);
            while event::poll(timeout)? {
                if let Event::Key(key) = event::read()?
                    && key.kind == KeyEventKind::Press
                    && let Some(go) = self.handle(key)
                {
                    // The next screens draw their own images: take the covers off kitty.
                    self.tiles.reset()?;
                    return Ok(go);
                }
                timeout = Duration::ZERO;
            }
        }
    }

    fn receive(&mut self, found: Found) {
        let current = found.query == self.query.trim();
        match found.result {
            Ok(links) => {
                self.cache.insert(found.query.clone(), links);
                if current {
                    self.show(found.query);
                }
            }
            Err(e) if current => self.status = tf("error", &[("e", &e)]),
            Err(_) => {}
        }
    }

    fn show(&mut self, query: String) {
        self.shown = query;
        self.list.select(Some(0));
    }

    fn ask(&mut self) {
        let query = self.query.trim();
        if query.chars().count() < MIN_CHARS
            || query == self.asked
            || self.cache.contains_key(query)
            || self.typed_at.elapsed() < DEBOUNCE
        {
            return;
        }
        self.asked = query.to_string();
        self.results.search(&self.asked);
    }

    fn edited(&mut self) {
        self.typed_at = Instant::now();
        self.status.clear();
        // After a failed search, typing the same query again must send it again.
        self.asked.clear();
        let query = self.query.trim().to_string();
        if self.cache.contains_key(&query) {
            self.show(query);
        }
    }

    /// Returns where to go, if the key leaves this screen.
    fn handle(&mut self, key: KeyEvent) -> Option<Go<Link>> {
        let key = match self.popups.key(key) {
            Shared::Quit => return Some(Go::Quit),
            Shared::Used => return None,
            Shared::Screen(key) => key,
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return Some(Go::Quit),
            KeyCode::Char('t') if ctrl => self.grid = !self.grid,
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
                if self.grid && self.geo.is_some() =>
            {
                let len = self.cache.get(&self.shown).map_or(0, Vec::len);
                let selected = self.list.selected().unwrap_or(0);
                let columns = self.tiles.columns;
                self.list
                    .select(Some(tiles::moved(selected, len, columns, key.code)));
            }
            KeyCode::Down => self.list.select_next(),
            KeyCode::Up => self.list.select_previous(),
            KeyCode::Enter => {
                let results = self.cache.get(&self.shown);
                if let Some(work) = self.list.selected().and_then(|i| results?.get(i)) {
                    return Some(Go::Next(work.clone()));
                }
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.edited();
            }
            KeyCode::Char(c) if !ctrl => {
                self.query.push(c);
                self.edited();
            }
            _ => {}
        }
        None
    }

    /// Draws the screen and returns where kitty must draw the covers.
    fn draw(&mut self, frame: &mut Frame) -> Vec<Placement> {
        let [input, results, status] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let block = Block::bordered().title(format!(" {} ", t("search.title")));
        frame.render_widget(Paragraph::new(self.query.as_str()).block(block), input);
        let cursor = input.x + 1 + self.query.chars().count() as u16;
        frame.set_cursor_position(Position::new(cursor, input.y + 1));

        let mut placements = Vec::new();
        match (self.cache.get(&self.shown), self.geo.filter(|_| self.grid)) {
            (Some(links), _) if links.is_empty() => {
                let text = format!(" {}", t("search.no_results"));
                frame.render_widget(Paragraph::new(text).dim(), results)
            }
            (Some(links), Some(geo)) => {
                let selected = self.list.selected().unwrap_or(0).min(links.len() - 1);
                placements = self.tiles.draw(frame, results, links, selected, geo);
            }
            (Some(links), None) => {
                let list = List::new(links.iter().map(|link| link.name.as_str()))
                    .highlight_style(Modifier::REVERSED);
                frame.render_stateful_widget(list, results, &mut self.list);
            }
            (None, _) => {}
        }

        let query = self.query.trim();
        let hint = if !self.status.is_empty() {
            self.status.as_str()
        } else if query.chars().count() >= MIN_CHARS && !self.cache.contains_key(query) {
            t("search.searching")
        } else {
            t("search.hint")
        };
        frame.render_widget(Paragraph::new(hint).dim(), status);

        self.popups.draw(frame, HELP);
        // Covers are drawn above the text: hide them while a popup is open.
        if self.popups.is_open() {
            Vec::new()
        } else {
            placements
        }
    }
}
