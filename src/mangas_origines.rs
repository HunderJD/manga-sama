//! Mangas-Origines client, a WordPress site (Madara theme).
//!
//! Search goes through the JSON endpoint of its search bar; chapters and pages are read from the
//! work's and the chapter's pages. Its `ajax/chapters` endpoint is behind Cloudflare's bot check:
//! never used.

use scraper::{Html, Selector};

use crate::Result;
use crate::http;
use crate::i18n::tf;
use crate::source::{Chapter, Work};

const BASE: &str = "https://mangas-origines.fr";

/// A work's id is the URL of its page.
pub fn search(query: &str) -> Result<Vec<Work>> {
    let url = format!("{BASE}/wp-admin/admin-ajax.php");
    let form = [("action", "wp-manga-search-manga"), ("title", query)];
    Ok(parse_search(&http::post_form(&url, &form)?))
}

/// A chapter's id is the URL of its page.
pub fn chapters(work: &str) -> Result<Vec<Chapter>> {
    Ok(parse_chapters(&http::text(work)?))
}

pub fn pages(chapter: &Chapter) -> Result<Vec<String>> {
    let pages = parse_pages(&http::text(&chapter.id)?);
    if pages.is_empty() {
        return Err(tf("source.no_pages", &[("url", &chapter.id)]).into());
    }
    Ok(pages)
}

/// `{"success": true, "data": [{"title", "url", "type"}]}`. Without results, `data` holds an
/// error message instead, which the filter drops.
fn parse_search(json: &str) -> Vec<Work> {
    let answer: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let works = answer["data"].as_array().into_iter().flatten();
    works
        .filter(|work| work["type"] == "manga")
        .filter_map(|work| {
            Some(Work {
                id: work["url"].as_str()?.to_string(),
                name: work["title"].as_str()?.to_string(),
                cover: None,
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
            Some((order, Chapter { id, label }))
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
            {"title":"Solo Leveling","url":"https://mangas-origines.fr/oeuvre/826-solo-leveling/","type":"manga"},
            {"title":"A novel","url":"https://mangas-origines.fr/oeuvre/novel/","type":"novel"}]}"#;
        let works = parse_search(json);
        let got: Vec<_> = works
            .iter()
            .map(|w| (w.name.as_str(), w.id.as_str()))
            .collect();
        let solo = (
            "Solo Leveling",
            "https://mangas-origines.fr/oeuvre/826-solo-leveling/",
        );
        assert_eq!(got, [solo]);
        let nothing =
            r#"{"success":false,"data":[{"error":"not found","message":"Pas de résultats"}]}"#;
        assert!(parse_search(nothing).is_empty());
    }

    #[test]
    fn chapter_list() {
        let html = r#"
            <div class="ori-chl-row" data-num="2" data-ordre="2"><a class="ori-chl-num" href="/c-2/"><b>2</b></a></div>
            <div class="ori-chl-row en-trop" data-num="1.5" data-ordre="1"><a class="ori-chl-num" href="/c-1-5/"><b>1.5</b></a></div>
            <div class="ori-chl-row en-trop" data-num="0" data-ordre="0"><a class="ori-chl-num" href="/c-0/"><b>0</b></a></div>"#;
        let chapters = parse_chapters(html);
        let got: Vec<_> = chapters
            .iter()
            .map(|c| (c.label.as_str(), c.id.as_str()))
            .collect();
        assert_eq!(got, [("0", "/c-0/"), ("1.5", "/c-1-5/"), ("2", "/c-2/")]);
    }

    #[test]
    fn page_images() {
        let html = r#"<img id="image-0" data-src=" https://mangas-origines.fr/01.jpg" class="wp-manga-chapter-img">
            <img id="image-1" data-src=" https://mangas-origines.fr/02.jpg" class="wp-manga-chapter-img">
            <img src="/logo.png">"#;
        assert_eq!(
            parse_pages(html),
            [
                "https://mangas-origines.fr/01.jpg",
                "https://mangas-origines.fr/02.jpg"
            ]
        );
    }

    #[test]
    #[ignore = "réseau"]
    fn full_walk() {
        let work = search("solo")
            .unwrap()
            .into_iter()
            .next()
            .expect("un résultat");
        let chapters = chapters(&work.id).unwrap();
        let urls = pages(&chapters[0]).unwrap();
        let bytes = http::bytes(&urls[0]).unwrap();
        image::load_from_memory(&bytes).unwrap();
    }
}
