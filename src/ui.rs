//! Shared pieces of the screens: where to go next, the keys every screen has (help, languages,
//! quit), the centered list popup, and the loading screen.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Constraint;
use ratatui::style::Modifier;
use ratatui::widgets::{Block, Clear, List, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use crate::Result;
use crate::i18n::{self, t, tf};

/// Where a screen sends the user next.
pub enum Go<T> {
    /// The next screen, with what was chosen.
    Next(T),
    /// The previous screen.
    Back,
    Quit,
}

/// Help lines every screen has, after its own: (keys, text key).
const SHARED_HELP: &[(&str, &str)] = &[("Ctrl+L", "help.languages"), ("?", "help.help")];

enum Popup {
    Help,
    Languages(ListState),
}

/// What is left of a key once the shared keys are handled.
pub enum Shared {
    Quit,
    /// Used by the popups: nothing left for the screen.
    Used,
    /// For the screen.
    Screen(KeyEvent),
}

/// The popups every screen has: its help (`?`) and the language list (Ctrl+L).
#[derive(Default)]
pub struct Popups {
    open: Option<Popup>,
}

impl Popups {
    /// Handles the keys shared by every screen: Ctrl+C quits, Ctrl+L and `?` open the popups,
    /// and an open popup takes every key. Other keys are left for the screen.
    pub fn key(&mut self, key: KeyEvent) -> Shared {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            return Shared::Quit;
        }
        if ctrl && key.code == KeyCode::Char('l') {
            self.open = match self.open {
                Some(Popup::Languages(_)) => None,
                _ => Some(Popup::Languages(
                    ListState::default().with_selected(Some(i18n::current())),
                )),
            };
            return Shared::Used;
        }
        match &mut self.open {
            None if key.code == KeyCode::Char('?') => self.open = Some(Popup::Help),
            None => return Shared::Screen(key),
            // Any key closes the help.
            Some(Popup::Help) => self.open = None,
            Some(Popup::Languages(list)) => match key.code {
                KeyCode::Char('j') | KeyCode::Down => list.select_next(),
                KeyCode::Char('k') | KeyCode::Up => list.select_previous(),
                KeyCode::Enter => {
                    i18n::set(list.selected().unwrap_or(0));
                    self.open = None;
                }
                KeyCode::Esc => self.open = None,
                _ => {}
            },
        }
        Shared::Used
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Draws the open popup, if any. `help` is the screen's own help: (keys, text key).
    pub fn draw(&mut self, frame: &mut Frame, help: &[(&str, &'static str)]) {
        match &mut self.open {
            None => {}
            Some(Popup::Help) => {
                let lines = help.iter().chain(SHARED_HELP);
                let width = lines.clone().map(|(keys, _)| keys.chars().count()).max();
                let width = width.unwrap_or(0);
                let lines = lines
                    .map(|(keys, text)| format!("{keys:<width$}  {}", t(text)))
                    .collect();
                popup(frame, t("help"), lines, &mut ListState::default());
            }
            Some(Popup::Languages(list)) => {
                let names = i18n::names().into_iter().map(String::from).collect();
                popup(frame, t("languages"), names, list);
            }
        }
    }
}

/// A bordered list in the middle of the screen.
pub fn popup(frame: &mut Frame, title: &str, items: Vec<String>, state: &mut ListState) {
    let longest = items
        .iter()
        .map(String::as_str)
        .chain([title])
        .map(|text| text.chars().count())
        .max();
    let width = longest.unwrap_or(0) as u16 + 4;
    let height = (items.len() as u16 + 2).min(frame.area().height * 4 / 5);
    let area = frame
        .area()
        .centered(Constraint::Length(width), Constraint::Length(height));
    let list = List::new(items)
        .block(Block::bordered().title(format!(" {title} ")))
        .highlight_style(Modifier::REVERSED);
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(list, area, state);
}

/// Runs `work` in a background thread and shows "Loading…" meanwhile, so the network never
/// freezes the app. Backspace or Esc gives up (a late answer is dropped). If `work` fails, its
/// error is shown until a key is pressed, then it's `Back`.
pub fn loading<T: Send + 'static>(
    terminal: &mut DefaultTerminal,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<Go<T>> {
    let (done, answer) = mpsc::channel();
    thread::spawn(move || {
        // Fails only if the screen gave up: nobody wants the answer any more.
        let _ = done.send(work());
    });
    let mut error = None;
    loop {
        match answer.try_recv() {
            Ok(Ok(value)) => return Ok(Go::Next(value)),
            Ok(Err(e)) => error = Some(e),
            // The thread ended without an answer: it panicked.
            Err(mpsc::TryRecvError::Disconnected) if error.is_none() => {
                error = Some("panic".to_string());
            }
            Err(_) => {}
        }
        let text = match &error {
            Some(e) => tf("error", &[("e", e)]),
            None => t("loading").to_string(),
        };
        terminal.draw(|frame| {
            let area = frame.area().centered_vertically(Constraint::Length(1));
            frame.render_widget(Paragraph::new(text).centered(), area);
        })?;
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if (ctrl && key.code == KeyCode::Char('c')) || key.code == KeyCode::Char('q') {
            return Ok(Go::Quit);
        }
        if error.is_some() || matches!(key.code, KeyCode::Backspace | KeyCode::Esc) {
            return Ok(Go::Back);
        }
    }
}
