//! The sites manga-sama reads from, and the types they all return.
//! Each site lives in its own module; `Source` only says which one to ask.

use crate::Result;
use crate::{anime_sama, mangas_origines};

/// A search result. `id` only means something to its source.
pub struct Work {
    pub id: String,
    pub name: String,
    /// Thumbnail URL.
    pub cover: Option<String>,
}

/// A scan version of a work (Anime-Sama: colour, black and white…).
pub struct Version {
    pub id: String,
    pub name: String,
}

/// A chapter. A work's chapters come in reading order.
#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    /// What its source needs to find the pages.
    pub id: String,
    /// What the reader shows: "1", "10.5".
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    AnimeSama,
    MangasOrigines,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::AnimeSama => "Anime-Sama",
            Source::MangasOrigines => "Mangas-Origines",
        }
    }

    /// The source Tab switches to.
    pub fn next(self) -> Self {
        match self {
            Source::AnimeSama => Source::MangasOrigines,
            Source::MangasOrigines => Source::AnimeSama,
        }
    }

    pub fn search(self, query: &str) -> Result<Vec<Work>> {
        match self {
            Source::AnimeSama => anime_sama::search(query),
            Source::MangasOrigines => mangas_origines::search(query),
        }
    }

    /// No version means no scans; with one, the reader opens it directly.
    pub fn versions(self, work: &str) -> Result<Vec<Version>> {
        match self {
            Source::AnimeSama => anime_sama::versions(work),
            Source::MangasOrigines => Ok(vec![Version {
                id: String::new(),
                name: String::new(),
            }]),
        }
    }

    pub fn chapters(self, work: &str, version: &str) -> Result<Vec<Chapter>> {
        match self {
            Source::AnimeSama => anime_sama::chapters(work, version),
            Source::MangasOrigines => mangas_origines::chapters(work),
        }
    }

    /// The URLs of the chapter's images, in order. Never empty.
    pub fn pages(self, chapter: &Chapter) -> Result<Vec<String>> {
        match self {
            Source::AnimeSama => anime_sama::pages(chapter),
            Source::MangasOrigines => mangas_origines::pages(chapter),
        }
    }
}
