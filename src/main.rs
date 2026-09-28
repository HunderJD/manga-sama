mod api;
mod i18n;
mod kitty;
mod pages;
mod reader;
mod search;
mod tiles;

use std::io::stdout;

use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture, KeyCode};
use ratatui::crossterm::execute;
use ratatui::layout::Constraint;
use ratatui::style::Modifier;
use ratatui::widgets::{Block, Clear, List, ListState};
use ratatui::{DefaultTerminal, Frame};

type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

fn main() {
    let mut terminal = ratatui::init();
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture);
        hook(info);
    }));
    let result = execute!(stdout(), EnableMouseCapture)
        .map_err(Into::into)
        .and_then(|()| run(&mut terminal));
    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
    kitty::cleanup();
    if let Err(e) = result {
        eprintln!("{}", i18n::tf("error", &[("e", &e)]));
        std::process::exit(1);
    }
}

fn run(terminal: &mut DefaultTerminal) -> Result<()> {
    let mut search = search::Search::new();
    let mut pages = pages::Pages::spawn();
    while let Some((title, chapters)) = search.run(terminal)? {
        if reader::run(terminal, &mut pages, title, chapters)? == reader::Exit::Quit {
            break;
        }
    }
    Ok(())
}

/// The language list opened with Ctrl+L, on the current language.
fn language_list() -> ListState {
    ListState::default().with_selected(Some(i18n::current()))
}

/// A key in the language list. Returns false once the list is closed.
fn pick_language(list: &mut ListState, code: KeyCode) -> bool {
    match code {
        KeyCode::Char('j') | KeyCode::Down => list.select_next(),
        KeyCode::Char('k') | KeyCode::Up => list.select_previous(),
        KeyCode::Enter => {
            i18n::set(list.selected().unwrap_or(0));
            return false;
        }
        KeyCode::Esc => return false,
        _ => {}
    }
    true
}

fn draw_languages(frame: &mut Frame, list: &mut ListState) {
    let names = i18n::names().into_iter().map(String::from).collect();
    popup(frame, i18n::t("languages"), names, list);
}

/// A bordered list in the middle of the screen: versions, chapters, help, languages.
fn popup(frame: &mut Frame, title: &str, items: Vec<String>, state: &mut ListState) {
    let longest = items
        .iter()
        .chain([&title.to_string()])
        .map(|i| i.chars().count())
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
