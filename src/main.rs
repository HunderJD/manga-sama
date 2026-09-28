mod api;
mod kitty;
mod pages;
mod reader;
mod search;

use std::io::stdout;

use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
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
    if let Err(e) = result {
        eprintln!("Erreur : {e}");
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

/// A bordered list in the middle of the screen: versions, chapters, help.
fn popup(frame: &mut Frame, title: &str, items: Vec<String>, state: &mut ListState) {
    let width = items.iter().map(|i| i.chars().count()).max().unwrap_or(0) as u16 + 4;
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
