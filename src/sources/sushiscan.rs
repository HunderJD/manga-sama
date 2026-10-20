//! The Sushiscan website, a WordPress site with the MangaReader theme. No versions, one scan per
//! work, volumes and chapters mixed in one list.
//!
//! Search goes through the JSON endpoint of its search bar (`ts_ac_do_search`, 10 results at most,
//! with covers); chapters are read from the work's HTML page, pages from the JSON the chapter's
//! page gives its reader script. Images (pages and covers) need a Referer from the site: see
//! `sources::image`.

use scraper::{Html, Selector};

use super::http::{get_text_cached, post_form};
use super::{Chapter, Link, Source};
use crate::Result;
use crate::i18n::tf;

pub const BASE: &str = "https://sushiscan.net";

pub struct Sushiscan;

impl Source for Sushiscan {
    fn name(&self) -> &'static str {
        "Sushiscan"
    }

    /// A work's `path` is its slug: history survives a change of domain.
    fn search(&self, query: &str) -> Result<Vec<Link>> {
        let url = format!("{BASE}/wp-admin/admin-ajax.php");
        let form = [("action", "ts_ac_do_search"), ("ts_ac_query", query)];
        Ok(parse_search(&post_form(&url, &form)?))
    }

    /// A chapter's id is its slug.
    fn chapters(&self, work: &Link, _version: &Link) -> Result<Vec<Chapter>> {
        let url = format!("{BASE}/catalogue/{}/", work.path);
        let chapters = parse_chapters(&get_text_cached(&url)?);
        if chapters.is_empty() {
            return Err(tf("sushiscan.no_chapters", &[("url", &url)]).into());
        }
        Ok(chapters)
    }

    fn pages(&self, chapter: &Chapter) -> Result<Vec<String>> {
        let url = format!("{BASE}/{}/", chapter.id);
        let pages = parse_pages(&get_text_cached(&url)?);
        if pages.is_empty() {
            return Err(tf("sushiscan.no_pages", &[("url", &url)]).into());
        }
        Ok(pages)
    }
}

/// The last part of a URL: "https://sushiscan.net/catalogue/one-piece/" gives "one-piece".
fn slug(url: &str) -> &str {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default()
}

/// `{"series": [{"all": [{"post_title", "post_link", "post_image", …}], …}]}`, `all` empty without
/// results.
fn parse_search(json: &str) -> Vec<Link> {
    let answer: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let works = answer["series"][0]["all"].as_array().into_iter().flatten();
    works
        .filter_map(|work| {
            Some(Link {
                name: work["post_title"].as_str()?.to_string(),
                path: slug(work["post_link"].as_str()?).to_string(),
                cover: work["post_image"].as_str().map(String::from),
            })
        })
        .collect()
}

/// One `li[data-num]` per chapter ("Chapitre 1180", "Volume 97"), newest first. Chapters lose
/// their "Chapitre " to read "Chapter 1180" on screen; volumes keep their word.
fn parse_chapters(html: &str) -> Vec<Chapter> {
    let doc = Html::parse_document(html);
    let row = Selector::parse("#chapterlist li[data-num]").unwrap();
    let link = Selector::parse("a").unwrap();
    let mut chapters: Vec<Chapter> = doc
        .select(&row)
        .filter_map(|row| {
            let num = row.attr("data-num")?;
            let href = row.select(&link).next()?.attr("href")?;
            Some(Chapter {
                label: num.trim_start_matches("Chapitre ").to_string(),
                id: slug(href).to_string(),
            })
        })
        .collect();
    chapters.reverse();
    chapters
}

/// The images are not in the HTML: the page gives them to its reader script, as
/// `ts_reader.run({…, "sources": [{"source": "Server 1", "images": [urls]}], …});`.
fn parse_pages(html: &str) -> Vec<String> {
    let Some((_, call)) = html.split_once("ts_reader.run(") else {
        return Vec::new();
    };
    // Reads the object alone, the `);` and the rest of the page after it are left.
    let reader = serde_json::Deserializer::from_str(call)
        .into_iter::<serde_json::Value>()
        .next()
        .and_then(|reader| reader.ok())
        .unwrap_or_default();
    let images = reader["sources"][0]["images"]
        .as_array()
        .into_iter()
        .flatten();
    images
        .filter_map(|image| image.as_str().map(String::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_results() {
        let json = r#"{"series":[{"all":[
            {"ID":67330,"post_image":"https:\/\/sushiscan.net\/wp-content\/uploads\/LegendairesStoriesCover.webp","post_image_html":"<div class=\"post-thumbnail\"><img src=\"https:\/\/sushiscan.net\/wp-content\/uploads\/LegendairesStoriesCover.webp\" alt=\"Les Légendaires - Stories\" width=\"50\" height=\"75\"><\/div>","post_title":"Les Légendaires - Stories","post_genres":"Aventure, Fantastique, Fantasy","post_type":"Bande-Dessinée","post_status":"En Cours","post_link":"https:\/\/sushiscan.net\/catalogue\/les-legendaires-stories\/","post_latest":"Volume 5","chapter_label":""},
            {"ID":50258,"post_image":"https:\/\/sushiscan.net\/wp-content\/uploads\/LesLegendairesCover.jpg","post_title":"Les Légendaires","post_link":"https:\/\/sushiscan.net\/catalogue\/les-legendaires\/","post_latest":"Volume 23","chapter_label":""}],
            "template":"<a href=\"{post_link}\">{post_image_html}<\/a>","title":"Search","class_name":"live-search_item"}]}"#;
        let works = parse_search(json);
        let got: Vec<_> = works
            .iter()
            .map(|w| (w.name.as_str(), w.path.as_str(), w.cover.as_deref()))
            .collect();
        let uploads = "https://sushiscan.net/wp-content/uploads";
        assert_eq!(
            got,
            [
                (
                    "Les Légendaires - Stories",
                    "les-legendaires-stories",
                    Some(&*format!("{uploads}/LegendairesStoriesCover.webp"))
                ),
                (
                    "Les Légendaires",
                    "les-legendaires",
                    Some(&format!("{uploads}/LesLegendairesCover.jpg"))
                )
            ]
        );
        let empty = r#"{"series":[{"all":[],"template":"","title":"Search","class_name":"live-search_item"}]}"#;
        assert!(parse_search(empty).is_empty());
    }

    #[test]
    fn chapter_list() {
        let html = r#"<div class="eplister" id="chapterlist"><ul ><li data-num="Chapitre 1180"><div class="chbox"><div class="eph-num">
            <a href="https://sushiscan.net/one-piece-chapitre-1180/">
            <span class="chapternum"> Chapitre 1180</span>
            <span class="chapterdate">19 avril 2026</span>
            </a></div></div></li><li data-num="Chapitre 985"><div class="chbox"><div class="eph-num">
            <a href="https://sushiscan.net/one-piece-chapitre-985/">
            <span class="chapternum"> Chapitre 985</span>
            </a></div></div></li><li data-num="Volume 1" class="first-chapter"><div class="chbox"><div class="eph-num">
            <a href="https://sushiscan.net/one-piece-volume-1/">
            <span class="chapternum"> Volume 1</span>
            </a></div></div></li></ul></div>
            <div class="listupd"><div class="bs"><div class="bsx">
            <a href="https://sushiscan.net/catalogue/claymore/" title="Claymore"></a></div></div></div>"#;
        let chapters = parse_chapters(html);
        let got: Vec<_> = chapters
            .iter()
            .map(|c| (c.label.as_str(), c.id.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("Volume 1", "one-piece-volume-1"),
                ("985", "one-piece-chapitre-985"),
                ("1180", "one-piece-chapitre-1180")
            ]
        );
    }

    #[test]
    fn page_images() {
        let folder = "https://c.sushiscan.net/wp-content/uploads58";
        let html = r#"<div id="readerarea"></div><script>ts_reader.run({"post_id":50266,"noimagehtml":"<center><h4>PAS ENCORE DE SCAN<\/h4><\/center>","prevUrl":"","nextUrl":"https:\/\/sushiscan.net\/les-legendaires-volume-2\/","mode":"single","sources":[{"source":"Server 1","images":["https:\/\/c.sushiscan.net\/wp-content\/uploads58\/LesLegendairesTome1-001.jpg","https:\/\/c.sushiscan.net\/wp-content\/uploads58\/LesLegendairesTome1-002.jpg"]}],"lazyload":false,"defaultSource":"Server 1","protected":false,"unlock_token":null});</script> <script>var post_id = 50258;</script>"#;
        assert_eq!(
            parse_pages(html),
            [
                format!("{folder}/LesLegendairesTome1-001.jpg"),
                format!("{folder}/LesLegendairesTome1-002.jpg")
            ]
        );
        assert!(parse_pages("<div id=\"readerarea\"></div>").is_empty());
    }
}
