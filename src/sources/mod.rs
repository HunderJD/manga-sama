//! Where manga come from: one module per website, each one a `Source`. Sites built on the same
//! theme share its code (`origines`). Adding a website means a module and a line in `SOURCES`.

mod anime_sama;
mod hentai_origines;
mod http;
mod mangas_origines;
mod origines;
mod sushiscan;

use crate::Result;

/// Every source, in the order Tab goes through them on the search screen. The first one is where
/// the app starts.
pub static SOURCES: &[&dyn Source] = &[
    &anime_sama::AnimeSama,
    &mangas_origines::MANGAS_ORIGINES,
    &hentai_origines::HENTAI_ORIGINES,
    &sushiscan::Sushiscan,
];

/// Downloads an image of any source. Sushiscan's Cloudflare refuses its images (pages on
/// `c.sushiscan.net`, covers on `sushiscan.net`) without a Referer from the site, its trailing `/`
/// included.
pub fn image(url: &str) -> Result<Vec<u8>> {
    let referer = url
        .contains("sushiscan.net/")
        .then(|| format!("{}/", sushiscan::BASE));
    http::image(url, referer.as_deref())
}

/// A search result or a scan version. `path` only means something to its source.
#[derive(Clone, Default)]
pub struct Link {
    pub name: String,
    pub path: String,
    /// Thumbnail URL, for search results.
    pub cover: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    /// What the screens show: "12", "85.5".
    pub label: String,
    /// What its source needs to list the pages; only it reads it.
    pub id: String,
}

/// What manga-sama needs from a website.
pub trait Source: Sync {
    fn name(&self) -> &'static str;

    fn search(&self, query: &str) -> Result<Vec<Link>>;

    /// The scan versions of a work (color, black and white…). None means no scans. With a single
    /// one there is no version screen: that's the default, for sites without versions.
    fn versions(&self, _work: &Link) -> Result<Vec<Link>> {
        Ok(vec![Link::default()])
    }

    /// In reading order.
    fn chapters(&self, work: &Link, version: &Link) -> Result<Vec<Chapter>>;

    /// The URLs of the chapter's images, in reading order.
    fn pages(&self, chapter: &Chapter) -> Result<Vec<String>>;
}
