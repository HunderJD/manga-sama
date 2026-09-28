//! Anime-Sama client.
//!
//! Search, scan versions and work titles are read from the site's HTML: Anime-Sama has no JSON API
//! for them (its own JS only gets an HTML fragment for the search bar). Only the chapter list
//! (`get_nb_chap_et_img.php`) is JSON, and pages are plain image files.

use std::collections::BTreeMap;

use scraper::{Html, Selector};

use crate::Result;
use crate::http::{self, encode};
use crate::i18n::tf;
use crate::source::{Chapter, Version, Work};

const BASE: &str = "https://anime-sama.to";

/// A work's id is its slug, from `/catalogue/<slug>/`.
pub fn search(query: &str) -> Result<Vec<Work>> {
    let url = format!(
        "{BASE}/catalogue/?search={}&type%5B%5D=Scans",
        encode(query)
    );
    Ok(parse_search(&http::text(&url)?))
}

/// A version's id is its path under the work, like `scan/vf`.
pub fn versions(work: &str) -> Result<Vec<Version>> {
    let url = format!("{BASE}/catalogue/{work}/");
    Ok(parse_versions(&http::text(&url)?))
}

/// The chapter list is JSON, asked for with the work's exact title, read on the version's page.
/// A chapter's id is the folder of its images with their count, `…/s2/scans/<title>/<n>?pages=<count>`,
/// so that `pages` needs no request.
pub fn chapters(work: &str, version: &str) -> Result<Vec<Chapter>> {
    let url = format!("{BASE}/catalogue/{work}/{version}/");
    let title =
        parse_title(&http::text(&url)?).ok_or_else(|| tf("source.no_title", &[("url", &url)]))?;
    let url = format!(
        "{BASE}/s2/scans/get_nb_chap_et_img.php?oeuvre={}",
        encode(&title)
    );
    let folder = format!("{BASE}/s2/scans/{}", encode(&title));
    let chapters = parse_chapters(&http::text(&url)?)?
        .into_iter()
        .map(|(number, pages)| Chapter {
            id: format!("{folder}/{number}?pages={pages}"),
            label: number.to_string(),
        })
        .collect();
    Ok(chapters)
}

pub fn pages(chapter: &Chapter) -> Result<Vec<String>> {
    let (folder, count) = chapter
        .id
        .split_once("?pages=")
        .ok_or("not an Anime-Sama chapter")?;
    let count: u32 = count.parse()?;
    Ok((1..=count)
        .map(|page| format!("{folder}/{page}.jpg"))
        .collect())
}

fn parse_search(html: &str) -> Vec<Work> {
    let doc = Html::parse_document(html);
    let link = Selector::parse(r#".catalog-card a[href*="/catalogue/"]"#).unwrap();
    let name = Selector::parse(".card-title").unwrap();
    let cover = Selector::parse("img").unwrap();
    doc.select(&link)
        .filter_map(|a| {
            let slug = a
                .attr("href")?
                .split("/catalogue/")
                .nth(1)?
                .trim_end_matches('/');
            let name = a.select(&name).next()?.text().collect::<String>();
            let cover = a.select(&cover).next().and_then(|img| img.attr("src"));
            (!slug.is_empty()).then(|| Work {
                id: slug.to_string(),
                name: name.trim().to_string(),
                cover: cover.map(String::from),
            })
        })
        .collect()
}

/// Reads the `panneauScan("name", "path");` calls of a work's page.
fn parse_versions(html: &str) -> Vec<Version> {
    html.split("panneauScan(\"")
        .skip(1)
        .filter_map(|call| {
            let (name, rest) = call.split_once('"')?;
            let (_, rest) = rest.split_once('"')?;
            let (path, _) = rest.split_once('"')?;
            Some(Version {
                id: path.to_string(),
                name: name.to_string(),
            })
        })
        .filter(|v| (v.name.as_str(), v.id.as_str()) != ("nom", "url"))
        .collect()
}

// Not trimmed and HTML-escaped on purpose: the site sends `innerHTML` as is, and
// "Jujutsu Kaisen Modulo " only exists with its trailing space.
fn parse_title(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    let title = Selector::parse("#titreOeuvre").unwrap();
    doc.select(&title).next().map(|e| e.inner_html())
}

/// The chapters' numbers and page counts. Empty chapters are dropped: the reader could never show them.
fn parse_chapters(json: &str) -> Result<Vec<(u32, u32)>> {
    // An unknown work gets `{"error": "..."}` instead of a chapter map: it parses as empty
    // and ends in the error below, with the site's answer in it.
    let chapters: Vec<_> = serde_json::from_str::<BTreeMap<u32, u32>>(json)
        .unwrap_or_default()
        .into_iter()
        .filter(|&(_, pages)| pages > 0)
        .collect();
    if chapters.is_empty() {
        return Err(tf("source.bad_chapters", &[("json", &json)]).into());
    }
    Ok(chapters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_results() {
        let html = r#"<div id="list_catalog">
            <div class="catalog-card"><a href="https://anime-sama.to/catalogue/berserk">
                <img src="https://cdn.jsdelivr.net/berserk.webp"><h2 class="card-title">Berserk</h2></a></div>
            <div class="catalog-card"><a href="https://anime-sama.to/catalogue/one-piece/">
                <h2 class="card-title"> One Piece </h2></a></div>
            <a href="https://anime-sama.to/catalogue/">Catalogue</a>
        </div>"#;
        let works = parse_search(html);
        let got: Vec<_> = works
            .iter()
            .map(|w| (w.name.as_str(), w.id.as_str(), w.cover.as_deref()))
            .collect();
        let berserk = (
            "Berserk",
            "berserk",
            Some("https://cdn.jsdelivr.net/berserk.webp"),
        );
        assert_eq!(got, [berserk, ("One Piece", "one-piece", None)]);
    }

    #[test]
    fn scan_versions() {
        let html = r#"<script>
            function panneauScan(nom, url){ }
            /*panneauScan("nom", "url"); -> on en met autant qu'on veut à la ligne*/
            panneauScan("Scans (couleur)", "scan/vf");
            panneauScan("Scans (noir et blanc)", "scan_noir-et-blanc/vf");
        </script>"#;
        let versions = parse_versions(html);
        let got: Vec<_> = versions
            .iter()
            .map(|v| (v.name.as_str(), v.id.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("Scans (couleur)", "scan/vf"),
                ("Scans (noir et blanc)", "scan_noir-et-blanc/vf")
            ]
        );
    }

    #[test]
    fn work_title() {
        let html = r#"<h3 id="titreOeuvre" class="uppercase">Jujutsu Kaisen Modulo </h3>"#;
        assert_eq!(parse_title(html).as_deref(), Some("Jujutsu Kaisen Modulo "));
        assert_eq!(parse_title("<h3>rien</h3>"), None);
    }

    #[test]
    fn chapter_list() {
        let chapters = parse_chapters(r#"{"1":57,"2":25,"10":3}"#).unwrap();
        assert_eq!(chapters, [(1, 57), (2, 25), (10, 3)]);
        assert_eq!(parse_chapters(r#"{"1":5,"2":0}"#).unwrap(), [(1, 5)]);
        assert!(parse_chapters(r#"{"error":"Oeuvre 'x' not found"}"#).is_err());
        assert!(parse_chapters("{}").is_err());
    }

    #[test]
    fn page_urls() {
        let chapter = Chapter {
            id: format!("{BASE}/s2/scans/Berserk/1?pages=2"),
            label: "1".into(),
        };
        let urls = pages(&chapter).unwrap();
        assert_eq!(
            urls,
            [
                format!("{BASE}/s2/scans/Berserk/1/1.jpg"),
                format!("{BASE}/s2/scans/Berserk/1/2.jpg")
            ]
        );
    }

    #[test]
    #[ignore = "réseau"]
    fn full_walk() {
        let work = search("berserk")
            .unwrap()
            .into_iter()
            .find(|w| w.id == "berserk")
            .expect("berserk trouvé");
        let thumbnail = http::bytes(work.cover.as_deref().expect("a cover")).unwrap();
        image::load_from_memory(&thumbnail).unwrap();
        let version = versions(&work.id)
            .unwrap()
            .into_iter()
            .next()
            .expect("une version");
        let chapters = chapters(&work.id, &version.id).unwrap();
        let urls = pages(&chapters[0]).unwrap();
        let bytes = http::bytes(&urls[0]).unwrap();
        image::load_from_memory(&bytes).unwrap();
    }
}
