//! The sites built on WordPress with the Madara theme and its `child-origines` child theme: same
//! pages and same search, only the address changes. No versions, one scan per work.
//!
//! Search goes through the JSON endpoint of its search bar (`madara_child_search`, with covers);
//! chapters and pages are read from the work's and the chapter's HTML pages. Its `ajax/chapters`
//! endpoint is behind Cloudflare's bot check: never used.

use scraper::{Html, Selector};

use super::http::{get_text_cached, post_form};
use super::{Chapter, Link, Source};
use crate::Result;
use crate::i18n::tf;

pub struct Origines {
    pub name: &'static str,
    /// The site's address, without a trailing `/`.
    pub base: &'static str,
}

impl Source for Origines {
    fn name(&self) -> &'static str {
        self.name
    }

    /// A work's `path` is the URL of its page.
    fn search(&self, query: &str) -> Result<Vec<Link>> {
        let url = format!("{}/wp-admin/admin-ajax.php", self.base);
        let form = [("action", "madara_child_search"), ("term", query)];
        Ok(parse_search(&post_form(&url, &form)?))
    }

    /// A chapter's id is the URL of its page.
    fn chapters(&self, work: &Link, _version: &Link) -> Result<Vec<Chapter>> {
        let chapters = parse_chapters(&get_text_cached(&work.path)?);
        if chapters.is_empty() {
            return Err(tf("origines.no_chapters", &[("url", &work.path)]).into());
        }
        Ok(chapters)
    }

    fn pages(&self, chapter: &Chapter) -> Result<Vec<String>> {
        let pages = parse_pages(&get_text_cached(&chapter.id)?);
        if pages.is_empty() {
            return Err(tf("origines.no_pages", &[("url", &chapter.id)]).into());
        }
        Ok(pages)
    }
}

/// `{"success": true, "data": [{"title", "url", "thumb", "genres", "rating"}]}`, `data` empty
/// without results. `thumb` is a 150×150 crop of the cover, the one the site's search shows; the
/// site's own script expects it to be empty sometimes.
fn parse_search(json: &str) -> Vec<Link> {
    let answer: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let works = answer["data"].as_array().into_iter().flatten();
    works
        .filter_map(|work| {
            Some(Link {
                name: work["title"].as_str()?.to_string(),
                path: work["url"].as_str()?.to_string(),
                cover: work["thumb"]
                    .as_str()
                    .filter(|thumb| !thumb.is_empty())
                    .map(String::from),
            })
        })
        .collect()
}

/// One `.ori-chl-row` per chapter, with its label (`data-num`) and reading order (`data-ordre`):
/// the page lists them newest first.
fn parse_chapters(html: &str) -> Vec<Chapter> {
    let doc = Html::parse_document(html);
    let row = Selector::parse(".ori-chl-row").unwrap();
    let link = Selector::parse("a.ori-chl-num").unwrap();
    let mut chapters: Vec<(u32, Chapter)> = doc
        .select(&row)
        .filter_map(|row| {
            let order = row.attr("data-ordre")?.parse().ok()?;
            let label = row.attr("data-num")?.to_string();
            let id = row.select(&link).next()?.attr("href")?.to_string();
            Some((order, Chapter { label, id }))
        })
        .collect();
    chapters.sort_by_key(|(order, _)| *order);
    chapters.into_iter().map(|(_, chapter)| chapter).collect()
}

/// The chapter's images. Their URL is in `data-src` (lazy loading), with a leading space.
fn parse_pages(html: &str) -> Vec<String> {
    let doc = Html::parse_document(html);
    let image = Selector::parse("img.wp-manga-chapter-img").unwrap();
    doc.select(&image)
        .filter_map(|img| img.attr("data-src").or(img.attr("src")))
        .map(|src| src.trim().to_string())
        .filter(|src| !src.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_results() {
        let json = r#"{"success":true,"data":[
            {"title":"Manager Kim","url":"https:\/\/mangas-origines.fr\/oeuvre\/manager-kim\/","thumb":"https:\/\/mangas-origines.fr\/wp-content\/uploads\/2023\/06\/manager-kim-v0-p2mvro0rhkvb1-1-150x150.jpg","genres":"Action \u00b7 Art Martiaux","rating":"4.0"},
            {"title":"Secr\u00e9taire En Mission","url":"https:\/\/mangas-origines.fr\/oeuvre\/secretaire-en-mission\/","thumb":"https:\/\/mangas-origines.fr\/wp-content\/uploads\/2025\/10\/thumbnail-11-150x150.jpg","genres":"Drame \u00b7 Manhwa","rating":"3.2"}]}"#;
        let works = parse_search(json);
        let got: Vec<_> = works
            .iter()
            .map(|w| (w.name.as_str(), w.path.as_str(), w.cover.as_deref()))
            .collect();
        let kim = (
            "Manager Kim",
            "https://mangas-origines.fr/oeuvre/manager-kim/",
            Some(
                "https://mangas-origines.fr/wp-content/uploads/2023/06/manager-kim-v0-p2mvro0rhkvb1-1-150x150.jpg",
            ),
        );
        let mission = (
            "Secrétaire En Mission",
            "https://mangas-origines.fr/oeuvre/secretaire-en-mission/",
            Some("https://mangas-origines.fr/wp-content/uploads/2025/10/thumbnail-11-150x150.jpg"),
        );
        assert_eq!(got, [kim, mission]);
        assert!(parse_search(r#"{"success":true,"data":[]}"#).is_empty());
    }

    #[test]
    fn chapter_list() {
        let html = r#"<div class="ori-chl-liste">
            <div class="ori-chl-row" data-chapitre="165609" data-nom="chapitre 207" data-num="207" data-ordre="231">
                <a class="ori-chl-num" href="https://mangas-origines.fr/oeuvre/manager-kim/chapitre-207/" tabindex="-1" aria-hidden="true"><b>207</b></a>
                <a class="ori-chl-corps" href="https://mangas-origines.fr/oeuvre/manager-kim/chapitre-207/"></a></div>
            <div class="ori-chl-row en-trop" data-chapitre="78077" data-nom=" chapitre 85.5" data-num="85.5" data-ordre="109">
                <a class="ori-chl-num" href="https://mangas-origines.fr/oeuvre/manager-kim/chapitre-85-5/"><b>85.5</b></a></div>
            <div class="ori-chl-row en-trop" data-chapitre="13176" data-nom="chapitre 1" data-num="1" data-ordre="0">
                <a class="ori-chl-num" href="https://mangas-origines.fr/oeuvre/manager-kim/chapitre-1/"><b>1</b></a></div>
        </div>"#;
        let chapters = parse_chapters(html);
        let got: Vec<_> = chapters
            .iter()
            .map(|c| (c.label.as_str(), c.id.as_str()))
            .collect();
        let chapter = "https://mangas-origines.fr/oeuvre/manager-kim/chapitre";
        assert_eq!(
            got,
            [
                ("1", &*format!("{chapter}-1/")),
                ("85.5", &format!("{chapter}-85-5/")),
                ("207", &format!("{chapter}-207/"))
            ]
        );
    }

    #[test]
    fn page_images() {
        let folder = "https://mangas-origines.fr/wp-content/uploads/WP-manga/data/manga_649c4fcc77c73/3a1bbb13fc5bbca85c48c3dbe46f249c";
        let html = format!(
            r#"<img id="image-0" data-src=" {folder}/01.webp" class="wp-manga-chapter-img img-responsive lazyload-ordered effect-fade">
            <img id="image-1" data-src=" {folder}/02.jpg" class="wp-manga-chapter-img img-responsive lazyload-ordered effect-fade">
            <img class="img-responsive" src="https://mangas-origines.fr/wp-content/uploads/2025/11/logos_oritest2-2.png">"#
        );
        assert_eq!(
            parse_pages(&html),
            [format!("{folder}/01.webp"), format!("{folder}/02.jpg")]
        );
    }
}
