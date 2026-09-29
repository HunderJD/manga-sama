//! Entry point: sets up the terminal, then chains the screens:
//! search → versions → chapters → reader, each one able to go back to the previous one.

mod chapters;
mod i18n;
mod kitty;
mod reader;
mod search;
mod sources;
mod ui;
mod versions;

use std::io::{IsTerminal, stdout};
use std::thread;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;

use crate::i18n::t;
use crate::reader::page_loader::PageLoader;
use crate::sources::anime_sama;
use crate::ui::Go;

type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

fn main() {
    let mut terminal = ratatui::init();
    // ratatui's hook restores the terminal. Only the main thread may do that: a background thread
    // that panics ends alone, and the app keeps running.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if thread::current().name() == Some("main") {
            let _ = execute!(stdout(), DisableMouseCapture);
            kitty::cleanup();
            hook(info);
        } else if !std::io::stderr().is_terminal() {
            eprintln!("{info}");
        }
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

/// The screens, one after the other. `continue 'screen` goes back to that screen.
fn run(terminal: &mut DefaultTerminal) -> Result<()> {
    let mut search = search::Search::new();
    let mut loader = PageLoader::spawn();
    'search: loop {
        let work = match search.run(terminal)? {
            Go::Next(work) => work,
            Go::Back | Go::Quit => return Ok(()),
        };

        let slug = work.path.clone();
        let versions = match ui::loading(terminal, move || {
            let versions = anime_sama::versions(&slug).map_err(|e| e.to_string())?;
            if versions.is_empty() {
                return Err(t("search.no_scans").to_string());
            }
            Ok(versions)
        })? {
            Go::Next(versions) => versions,
            Go::Back => continue 'search,
            Go::Quit => return Ok(()),
        };
        // With a single version there is no version screen: going back goes to the search.
        let single = versions.len() == 1;
        let mut version = 0;

        'versions: loop {
            if !single {
                version = match versions::run(terminal, &work.name, &versions, version)? {
                    Go::Next(version) => version,
                    Go::Back => continue 'search,
                    Go::Quit => return Ok(()),
                };
            }

            let (slug, path) = (work.path.clone(), versions[version].path.clone());
            let (title, chapters) = match ui::loading(terminal, move || {
                let title = anime_sama::title(&slug, &path).map_err(|e| e.to_string())?;
                let chapters = anime_sama::chapters(&title).map_err(|e| e.to_string())?;
                Ok((title, chapters))
            })? {
                Go::Next(loaded) => loaded,
                Go::Back if single => continue 'search,
                Go::Back => continue 'versions,
                Go::Quit => return Ok(()),
            };

            let mut chapter = 0;
            loop {
                chapter = match chapters::run(terminal, &work.name, &chapters, chapter)? {
                    Go::Next(chapter) => chapter,
                    Go::Back if single => continue 'search,
                    Go::Back => continue 'versions,
                    Go::Quit => return Ok(()),
                };
                // Back from the reader: the chapter list, on the chapter being read.
                chapter = match reader::run(terminal, &mut loader, &title, &chapters, chapter)? {
                    Some(chapter) => chapter,
                    None => return Ok(()),
                };
            }
        }
    }
}
