//! The version screen: which scans of a work to read (e.g. color or black and white).

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::widgets::ListState;

use crate::Result;
use crate::i18n::t;
use crate::sources::anime_sama::Link;
use crate::ui::{self, Go, Popups, Shared};

const HELP: &[(&str, &str)] = &[
    ("↑ ↓ j k", "help.select"),
    ("Enter", "help.open"),
    ("Backspace Esc", "help.back"),
    ("q", "help.quit"),
];

/// A small list on a blank screen, titled "<work> · Version". Returns the index of the version.
pub fn run(
    terminal: &mut DefaultTerminal,
    work: &str,
    versions: &[Link],
    selected: usize,
) -> Result<Go<usize>> {
    let mut list = ListState::default().with_selected(Some(selected));
    let mut popups = Popups::default();
    let title = format!("{work} · {}", t("versions.title"));
    loop {
        terminal.draw(|frame| {
            let items = versions.iter().map(|link| link.name.clone()).collect();
            ui::popup(frame, &title, items, &mut list);
            popups.draw(frame, HELP);
        })?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let key = match popups.key(key) {
            Shared::Quit => return Ok(Go::Quit),
            Shared::Used => continue,
            Shared::Screen(key) => key,
        };
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => list.select_next(),
            KeyCode::Char('k') | KeyCode::Up => list.select_previous(),
            KeyCode::Enter => {
                // ListState only clamps its selection when rendered.
                let index = list.selected().unwrap_or(0).min(versions.len() - 1);
                return Ok(Go::Next(index));
            }
            KeyCode::Backspace | KeyCode::Esc => return Ok(Go::Back),
            KeyCode::Char('q') => return Ok(Go::Quit),
            _ => {}
        }
    }
}
