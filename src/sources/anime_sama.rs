//! The Anime-Sama website: search, scan versions, chapter lists and page URLs.
//!
//! Search, scan versions and work titles are read from the site's HTML: Anime-Sama has no JSON API
//! for them (its own JS only gets an HTML fragment for the search bar). Only the chapter list
//! (`get_nb_chap_et_img.php`) is JSON, and pages are plain image files.

use std::collections::BTreeMap;

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use scraper::{Html, Selector};

use super::http::{get_text, get_text_cached};
use super::{Chapter, Link, Source};
use crate::Result;
use crate::i18n::tf;

const BASE: &str = "https://anime-sama.to";

pub struct AnimeSama;

impl Source for AnimeSama {
    fn name(&self) -> &'static str {
        "Anime-Sama"
    }

    /// A work's `path` is its slug, a version's `path` its folder ("scan/vf").
    fn search(&self, query: &str) -> Result<Vec<Link>> {
        let url = format!(
            "{BASE}/catalogue/?search={}&type%5B%5D=Scans",
            url_encode(query)
        );
        Ok(parse_search(&get_text(&url)?))
    }

    fn versions(&self, work: &Link) -> Result<Vec<Link>> {
        let url = format!("{BASE}/catalogue/{}/", work.path);
        Ok(parse_versions(&get_text_cached(&url)?))
    }

    fn chapters(&self, work: &Link, version: &Link) -> Result<Vec<Chapter>> {
        let title = title(&work.path, &version.path)?;
        let url = format!(
            "{BASE}/s2/scans/get_nb_chap_et_img.php?oeuvre={}",
            url_encode(&title)
        );
        parse_chapters(&title, &get_text_cached(&url)?)
    }

    /// Pages are numbered files in the chapter's folder, given by the id (see `parse_chapters`):
    /// no request needed.
    fn pages(&self, chapter: &Chapter) -> Result<Vec<String>> {
        let (folder, pages) = chapter.id.rsplit_once('/').ok_or("bad chapter id")?;
        let pages: u32 = pages.parse()?;
        Ok((1..=pages)
            .map(|page| format!("{BASE}/s2/scans/{folder}/{page}.jpg"))
            .collect())
    }
}

fn url_encode(s: &str) -> String {
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

/// The work's name as the chapter API expects it.
fn title(slug: &str, path: &str) -> Result<String> {
    let url = format!("{BASE}/catalogue/{slug}/{path}/");
    parse_title(&get_text_cached(&url)?)
        .ok_or_else(|| tf("anime_sama.no_title", &[("url", &url)]).into())
}

fn parse_search(html: &str) -> Vec<Link> {
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
            (!slug.is_empty()).then(|| Link {
                name: name.trim().to_string(),
                path: slug.to_string(),
                cover: cover.map(String::from),
            })
        })
        .collect()
}

/// Reads the `panneauScan("name", "path");` calls of a work's page.
fn parse_versions(html: &str) -> Vec<Link> {
    html.split("panneauScan(\"")
        .skip(1)
        .filter_map(|call| {
            let (name, rest) = call.split_once('"')?;
            let (_, rest) = rest.split_once('"')?;
            let (path, _) = rest.split_once('"')?;
            Some(Link {
                name: name.to_string(),
                path: path.to_string(),
                cover: None,
            })
        })
        // The site leaves an example `panneauScan("nom", "url")` call in a comment.
        .filter(|v| (v.name.as_str(), v.path.as_str()) != ("nom", "url"))
        .collect()
}

/// Kept as is (not trimmed, still HTML-escaped) on purpose: the site sends `innerHTML` as is,
/// and "Jujutsu Kaisen Modulo " only exists with its trailing space.
fn parse_title(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    let title = Selector::parse("#titreOeuvre").unwrap();
    doc.select(&title).next().map(|e| e.inner_html())
}

/// Empty chapters are dropped: the reader could never show them. A chapter's id is its folder
/// and its page count, `<title>/<number>/<pages>`: the title is percent-encoded, so it has no `/`.
fn parse_chapters(title: &str, json: &str) -> Result<Vec<Chapter>> {
    // An unknown work gets `{"error": "..."}` instead of a chapter map: it parses as empty
    // and ends in the error below, with the site's answer in it.
    let chapters: Vec<_> = serde_json::from_str::<BTreeMap<u32, u32>>(json)
        .unwrap_or_default()
        .into_iter()
        .filter(|&(_, pages)| pages > 0)
        .map(|(number, pages)| Chapter {
            label: number.to_string(),
            id: format!("{}/{number}/{pages}", url_encode(title)),
        })
        .collect();
    if chapters.is_empty() {
        return Err(tf("anime_sama.bad_chapters", &[("json", &json)]).into());
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
                <img loading="lazy" class="card-image" src="https://cdn.jsdelivr.net/gh/Anime-Sama/IMG@img/contenu/thumb/berserk0.webp" alt="Berserk">
                <h2 class="card-title">Berserk</h2></a></div>
            <div class="catalog-card"><a href="https://anime-sama.to/catalogue/berserk-of-gluttony/">
                <h2 class="card-title"> Berserk of Gluttony </h2></a></div>
            <a href="https://anime-sama.to/catalogue/">Catalogue</a>
        </div>"#;
        let works = parse_search(html);
        let got: Vec<_> = works
            .iter()
            .map(|w| (w.name.as_str(), w.path.as_str(), w.cover.as_deref()))
            .collect();
        let berserk = (
            "Berserk",
            "berserk",
            Some("https://cdn.jsdelivr.net/gh/Anime-Sama/IMG@img/contenu/thumb/berserk0.webp"),
        );
        let gluttony = ("Berserk of Gluttony", "berserk-of-gluttony", None);
        assert_eq!(got, [berserk, gluttony]);
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
            .map(|v| (v.name.as_str(), v.path.as_str()))
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
        assert_eq!(parse_title("<h3>nothing</h3>"), None);
    }

    #[test]
    fn chapter_list() {
        let chapters = parse_chapters("Fate/Zero ", r#"{"1":57,"2":0,"10":3}"#).unwrap();
        let got: Vec<_> = chapters
            .iter()
            .map(|c| (c.label.as_str(), c.id.as_str()))
            .collect();
        let zero = "Fate%2FZero%20";
        assert_eq!(
            got,
            [
                ("1", &*format!("{zero}/1/57")),
                ("10", &format!("{zero}/10/3"))
            ]
        );
        assert!(parse_chapters("x", r#"{"error":"Oeuvre 'x' not found"}"#).is_err());
        assert!(parse_chapters("x", "{}").is_err());
    }

    #[test]
    fn page_urls() {
        let chapter = Chapter {
            label: "10".to_string(),
            id: "Fate%2FZero%20/10/2".to_string(),
        };
        let folder = "https://anime-sama.to/s2/scans/Fate%2FZero%20/10";
        assert_eq!(
            AnimeSama.pages(&chapter).unwrap(),
            [format!("{folder}/1.jpg"), format!("{folder}/2.jpg")]
        );
    }
}
