use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Position};
use ratatui::style::{Modifier, Stylize};
use ratatui::widgets::{Block, List, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use crate::Result;
use crate::api::{self, Chapter, Link};
use crate::i18n::{t, tf};
use crate::kitty::{self, Geo, Placement};
use crate::tiles::{self, Tiles};

/// Time without typing before searching.
const DEBOUNCE: Duration = Duration::from_millis(350);
/// One letter would list the whole catalogue.
const MIN_CHARS: usize = 2;

/// A query and its results, from the search thread.
struct Found {
    query: String,
    result: Result<Vec<Link>, String>,
}

/// What to fetch once "Chargement…" is on screen.
enum Todo {
    Versions { slug: String, name: String },
    Chapters { slug: String, path: String },
}

/// The scan versions of the chosen work, when it has several.
struct Versions {
    slug: String,
    name: String,
    links: Vec<Link>,
    list: ListState,
}

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
    versions: Option<Versions>,
    todo: Option<Todo>,
    status: String,
    /// The help popup is open.
    help: bool,
    /// The language list, when open.
    languages: Option<ListState>,
    /// Results as cover tiles instead of a list; Ctrl+T switches.
    grid: bool,
    tiles: Tiles,
    /// Terminal size; `None` when it gives no pixel size, then there are no covers.
    geo: Option<Geo>,
    queries: Sender<String>,
    found: Receiver<Found>,
}

impl Search {
    pub fn new() -> Self {
        let (queries, query_rx) = mpsc::channel();
        let (found_tx, found) = mpsc::channel();
        thread::spawn(move || search(query_rx, found_tx));
        Search {
            query: String::new(),
            typed_at: Instant::now(),
            asked: String::new(),
            cache: HashMap::new(),
            shown: String::new(),
            list: ListState::default(),
            versions: None,
            todo: None,
            status: String::new(),
            help: false,
            languages: None,
            grid: false,
            tiles: Tiles::new(),
            geo: None,
            queries,
            found,
        }
    }

    /// Runs until a work is opened (its API title and chapters) or the user quits (`None`).
    pub fn run(
        &mut self,
        terminal: &mut DefaultTerminal,
    ) -> Result<Option<(String, Vec<Chapter>)>> {
        // What kitty shows now.
        let mut shown = Vec::new();
        loop {
            while let Ok(found) = self.found.try_recv() {
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
            if let Some(todo) = self.todo.take() {
                match self.fetch(todo) {
                    Ok(Some(work)) => {
                        // The reader draws its own images: take the covers off kitty.
                        self.tiles.reset()?;
                        return Ok(Some(work));
                    }
                    Ok(None) => {}
                    Err(e) => self.status = tf("error", &[("e", &e)]),
                }
                continue;
            }
            let mut timeout = Duration::from_millis(50);
            while event::poll(timeout)? {
                if self.handle(event::read()?) {
                    self.tiles.reset()?;
                    return Ok(None);
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
        // Fails only if the search thread is gone; the status then stays "Recherche…".
        let _ = self.queries.send(self.asked.clone());
    }

    fn edited(&mut self) {
        self.typed_at = Instant::now();
        self.status.clear();
        let query = self.query.trim().to_string();
        if self.cache.contains_key(&query) {
            self.show(query);
        }
    }

    fn fetch(&mut self, todo: Todo) -> Result<Option<(String, Vec<Chapter>)>> {
        match todo {
            Todo::Versions { slug, name } => {
                let mut links = api::versions(&slug)?;
                match links.len() {
                    0 => self.status = t("search.no_scans").into(),
                    1 => {
                        let path = links.remove(0).path;
                        self.todo = Some(Todo::Chapters { slug, path });
                    }
                    _ => {
                        self.status.clear();
                        let list = ListState::default().with_selected(Some(0));
                        self.versions = Some(Versions {
                            slug,
                            name,
                            links,
                            list,
                        });
                    }
                }
                Ok(None)
            }
            Todo::Chapters { slug, path } => {
                let title = api::title(&slug, &path)?;
                let chapters = api::chapters(&title)?;
                self.status.clear();
                Ok(Some((title, chapters)))
            }
        }
    }

    /// Returns true to quit.
    fn handle(&mut self, event: Event) -> bool {
        let Event::Key(key) = event else { return false };
        if key.kind != KeyEventKind::Press {
            return false;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            return true;
        }
        if ctrl && key.code == KeyCode::Char('l') {
            self.languages = match self.languages {
                Some(_) => None,
                None => Some(crate::language_list()),
            };
            return false;
        }
        if let Some(list) = &mut self.languages {
            if !crate::pick_language(list, key.code) {
                self.languages = None;
            }
            return false;
        }
        if self.help {
            // Any key closes the help.
            self.help = false;
            return false;
        }
        if self.versions.is_some() {
            self.handle_versions(key.code);
            return false;
        }
        match key.code {
            KeyCode::Esc => return true,
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
                    let (slug, name) = (work.path.clone(), work.name.clone());
                    self.todo = Some(Todo::Versions { slug, name });
                    self.status = t("loading").into();
                }
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.edited();
            }
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char(c) if !ctrl => {
                self.query.push(c);
                self.edited();
            }
            _ => {}
        }
        false
    }

    fn handle_versions(&mut self, code: KeyCode) {
        let Some(versions) = &mut self.versions else {
            return;
        };
        match code {
            KeyCode::Char('j') | KeyCode::Down => versions.list.select_next(),
            KeyCode::Char('k') | KeyCode::Up => versions.list.select_previous(),
            KeyCode::Enter => {
                let chosen = versions.list.selected().and_then(|i| versions.links.get(i));
                if let Some(version) = chosen {
                    let (slug, path) = (versions.slug.clone(), version.path.clone());
                    self.todo = Some(Todo::Chapters { slug, path });
                    self.status = t("loading").into();
                    self.versions = None;
                }
            }
            KeyCode::Esc | KeyCode::Backspace => self.versions = None,
            _ => {}
        }
    }

    /// Draws the screen and returns where kitty must draw the covers.
    fn draw(&mut self, frame: &mut Frame) -> Vec<Placement> {
        if let Some(versions) = &mut self.versions {
            // A screen of its own: "<work> · Version" and the choices.
            let title = format!(" {} · {} ", versions.name, t("search.version"));
            let list = List::new(versions.links.iter().map(|link| link.name.as_str()))
                .block(Block::bordered().title(title))
                .highlight_style(Modifier::REVERSED);
            frame.render_stateful_widget(list, frame.area(), &mut versions.list);
            if let Some(list) = &mut self.languages {
                crate::draw_languages(frame, list);
            }
            return Vec::new();
        }

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

        if self.help {
            let items = t("search.help").lines().map(String::from).collect();
            crate::popup(frame, t("help"), items, &mut ListState::default());
        }
        if let Some(list) = &mut self.languages {
            crate::draw_languages(frame, list);
        }
        // Covers are drawn above the text: hide them while a popup is open.
        let popup = self.help || self.languages.is_some();
        if popup { Vec::new() } else { placements }
    }
}

/// One search at a time; queries already outdated when it is free are skipped.
fn search(queries: Receiver<String>, found: Sender<Found>) -> Option<()> {
    loop {
        let query = queries.recv().ok()?;
        let query = queries.try_iter().last().unwrap_or(query);
        let result = api::search(&query).map_err(|e| e.to_string());
        found.send(Found { query, result }).ok()?;
    }
}
