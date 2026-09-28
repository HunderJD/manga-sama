mod api;
mod reader;

use std::io::stdout;

use inquire::{InquireError, Select, Text};
use ratatui::Frame;
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::layout::Constraint;
use ratatui::style::Modifier;
use ratatui::widgets::{Block, Clear, List, ListState};

type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

fn main() {
    match run() {
        Ok(()) => {}
        Err(e) if matches!(e.downcast_ref(), Some(InquireError::OperationInterrupted)) => {}
        Err(e) => {
            eprintln!("Erreur : {e}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<()> {
    let mut query = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    loop {
        if query.trim().is_empty() {
            match Text::new("Recherche :").prompt_skippable()? {
                Some(q) if !q.trim().is_empty() => query = q,
                _ => return Ok(()),
            }
        }
        let works = api::search(query.trim())?;
        query.clear();
        if works.is_empty() {
            println!("Aucun résultat.");
            continue;
        }
        let Some(work) = Select::new("Titre :", works).prompt_skippable()? else {
            return Ok(());
        };

        let mut versions = api::versions(&work.path)?;
        let version = match versions.len() {
            0 => {
                println!("Aucun scan pour ce titre.");
                continue;
            }
            1 => versions.pop(),
            _ => Select::new("Version :", versions).prompt_skippable()?,
        };
        let Some(version) = version else {
            return Ok(());
        };

        let title = api::title(&work.path, &version.path)?;
        let chapters = api::chapters(&title)?;
        return tui(title, chapters);
    }
}

fn tui(title: String, chapters: Vec<api::Chapter>) -> Result<()> {
    let mut terminal = ratatui::init();
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture);
        hook(info);
    }));
    let result = execute!(stdout(), EnableMouseCapture)
        .map_err(Into::into)
        .and_then(|()| reader::run(&mut terminal, &mut reader::Pages::spawn(), title, chapters));
    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
    result.map(|_| ())
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
