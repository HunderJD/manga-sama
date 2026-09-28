mod api;
mod reader;

use inquire::{InquireError, Select, Text};

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
        return reader::run(title, chapters);
    }
}
