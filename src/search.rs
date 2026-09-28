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

/// Time without typing before searching, like the site's search bar.
const DEBOUNCE: Duration = Duration::from_millis(200);

/// A query and its results, from the search thread.
type Found = (String, Result<Vec<Link>, String>);

/// What to fetch once "Chargement…" is on screen.
enum Todo {
    Versions(String),
    Chapters { slug: String, path: String },
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
    /// Slug and scan versions of the chosen work, when it has several.
    versions: Option<(String, Vec<Link>, ListState)>,
    todo: Option<Todo>,
    status: String,
    queries: Sender<String>,
    found: Receiver<Found>,
}

impl Search {
    pub fn new(query: String) -> Self {
        let (queries, query_rx) = mpsc::channel();
        let (found_tx, found) = mpsc::channel();
        thread::spawn(move || search(query_rx, found_tx));
        Search {
            query,
            typed_at: Instant::now(),
            asked: String::new(),
            cache: HashMap::new(),
            shown: String::new(),
            list: ListState::default(),
            versions: None,
            todo: None,
            status: String::new(),
            queries,
            found,
        }
    }

    /// Runs until a work is opened (its API title and chapters) or the user quits (`None`).
    pub fn run(
        &mut self,
        terminal: &mut DefaultTerminal,
    ) -> Result<Option<(String, Vec<Chapter>)>> {
        loop {
            let found: Vec<Found> = self.found.try_iter().collect();
            for (query, result) in found {
                self.receive(query, result);
            }
            self.ask();
            terminal.draw(|frame| self.draw(frame))?;
            if let Some(todo) = self.todo.take() {
                match self.fetch(todo) {
                    Ok(Some(work)) => return Ok(Some(work)),
                    Ok(None) => {}
                    Err(e) => self.status = format!("Erreur : {e}"),
                }
                continue;
            }
            let mut timeout = Duration::from_millis(50);
            while event::poll(timeout)? {
                if self.handle(event::read()?) {
                    return Ok(None);
                }
                timeout = Duration::ZERO;
            }
        }
    }

    fn receive(&mut self, query: String, result: Result<Vec<Link>, String>) {
        let current = query == self.query.trim();
        match result {
            Ok(links) => {
                self.cache.insert(query.clone(), links);
                if current {
                    self.show(query);
                }
            }
            Err(e) if current => self.status = format!("Erreur : {e}"),
            Err(_) => {}
        }
    }

    fn show(&mut self, query: String) {
        self.shown = query;
        self.list.select(Some(0));
    }

    fn ask(&mut self) {
        let query = self.query.trim();
        if query.is_empty()
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
            Todo::Versions(slug) => {
                let mut versions = api::versions(&slug)?;
                match versions.len() {
                    0 => self.status = "Aucun scan pour ce titre.".into(),
                    1 => {
                        let path = versions.remove(0).path;
                        self.todo = Some(Todo::Chapters { slug, path });
                    }
                    _ => {
                        self.status.clear();
                        let list = ListState::default().with_selected(Some(0));
                        self.versions = Some((slug, versions, list));
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
        if self.versions.is_some() {
            self.handle_versions(key.code);
            return false;
        }
        match key.code {
            KeyCode::Esc => return true,
            KeyCode::Down => self.list.select_next(),
            KeyCode::Up => self.list.select_previous(),
            KeyCode::Enter => {
                let results = self.cache.get(&self.shown);
                if let Some(work) = self.list.selected().and_then(|i| results?.get(i)) {
                    self.todo = Some(Todo::Versions(work.path.clone()));
                    self.status = "Chargement…".into();
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
        false
    }

    fn handle_versions(&mut self, code: KeyCode) {
        let Some((slug, versions, list)) = &mut self.versions else {
            return;
        };
        match code {
            KeyCode::Char('j') | KeyCode::Down => list.select_next(),
            KeyCode::Char('k') | KeyCode::Up => list.select_previous(),
            KeyCode::Enter => {
                if let Some(version) = list.selected().and_then(|i| versions.get(i)) {
                    let (slug, path) = (slug.clone(), version.path.clone());
                    self.todo = Some(Todo::Chapters { slug, path });
                    self.status = "Chargement…".into();
                    self.versions = None;
                }
            }
            KeyCode::Esc => self.versions = None,
            _ => {}
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let [input, results, status] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let block = Block::bordered().title(" Recherche ");
        frame.render_widget(Paragraph::new(self.query.as_str()).block(block), input);
        let cursor = input.x + 1 + self.query.chars().count() as u16;
        frame.set_cursor_position(Position::new(cursor, input.y + 1));

        match self.cache.get(&self.shown) {
            Some(links) if links.is_empty() => {
                frame.render_widget(Paragraph::new(" Aucun résultat.").dim(), results)
            }
            Some(links) => {
                let list = List::new(links.iter().map(|link| link.name.as_str()))
                    .highlight_style(Modifier::REVERSED);
                frame.render_stateful_widget(list, results, &mut self.list);
            }
            None => {}
        }

        let query = self.query.trim();
        let hint = if !self.status.is_empty() {
            self.status.as_str()
        } else if !query.is_empty() && !self.cache.contains_key(query) {
            "Recherche…"
        } else {
            "↑↓ choisir · Entrée ouvrir · Échap quitter"
        };
        frame.render_widget(Paragraph::new(hint).dim(), status);

        if let Some((_, versions, list)) = &mut self.versions {
            let items = versions.iter().map(|v| v.name.clone()).collect();
            crate::popup(frame, "Version", items, list);
        }
    }
}

/// One search at a time; queries already outdated when it is free are skipped.
fn search(queries: Receiver<String>, found: Sender<Found>) {
    while let Ok(query) = queries.recv() {
        let query = queries.try_iter().last().unwrap_or(query);
        let result = api::search(&query).map_err(|e| e.to_string());
        if found.send((query, result)).is_err() {
            return;
        }
    }
}
