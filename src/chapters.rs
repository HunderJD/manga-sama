//! The chapter screen: pick the chapter to read, or type its number to filter the list.

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::widgets::ListState;

use crate::Result;
use crate::i18n::{t, tf};
use crate::sources::anime_sama::Chapter;
use crate::ui::{self, Go, Popups, Shared};

const HELP: &[(&str, &str)] = &[
    ("0-9", "help.filter"),
    ("↑ ↓ j k", "help.select"),
    ("Enter", "help.read"),
    ("Backspace Esc", "help.back"),
    ("q", "help.quit"),
];

/// A list on a blank screen, titled "<work> · Chapters" and the number typed so far, opening on
/// `selected`. Returns the index of the chapter in `chapters`.
pub fn run(
    terminal: &mut DefaultTerminal,
    work: &str,
    chapters: &[Chapter],
    selected: usize,
) -> Result<Go<usize>> {
    let mut list = ListState::default().with_selected(Some(selected));
    let mut popups = Popups::default();
    // The chapter number typed so far.
    let mut filter = String::new();
    loop {
        let shown = matching(chapters, &filter);
        terminal.draw(|frame| {
            let items = shown
                .iter()
                .map(|&i| tf("chapters.item", &[("n", &chapters[i].number)]))
                .collect();
            let title = format!("{work} · {} {filter}", t("chapters.title"));
            ui::popup(frame, title.trim_end(), items, &mut list);
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
            KeyCode::Char(digit) if digit.is_ascii_digit() => {
                filter.push(digit);
                list.select(Some(0));
            }
            // Erases a typed digit first, then goes back.
            KeyCode::Backspace if !filter.is_empty() => {
                filter.pop();
                list.select(Some(0));
            }
            KeyCode::Backspace | KeyCode::Esc => return Ok(Go::Back),
            KeyCode::Char('q') => return Ok(Go::Quit),
            KeyCode::Char('j') | KeyCode::Down => list.select_next(),
            KeyCode::Char('k') | KeyCode::Up => list.select_previous(),
            KeyCode::Enter => {
                // ListState only clamps its selection when rendered.
                let index = list
                    .selected()
                    .unwrap_or(0)
                    .min(shown.len().saturating_sub(1));
                if let Some(&chapter) = shown.get(index) {
                    return Ok(Go::Next(chapter));
                }
            }
            _ => {}
        }
    }
}

/// Indexes of the chapters whose number starts with `filter`.
fn matching(chapters: &[Chapter], filter: &str) -> Vec<usize> {
    let starts = |c: &Chapter| c.number.to_string().starts_with(filter);
    (0..chapters.len())
        .filter(|&i| starts(&chapters[i]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_filter() {
        let chapters = [1, 2, 12, 21, 120].map(|number| Chapter { number, pages: 1 });
        assert_eq!(matching(&chapters, ""), [0, 1, 2, 3, 4]);
        assert_eq!(matching(&chapters, "12"), [2, 4]);
        assert!(matching(&chapters, "9").is_empty());
    }
}
