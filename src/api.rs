use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use scraper::{Html, Selector};

use crate::Result;

const BASE: &str = "https://anime-sama.to";

static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::Agent::config_builder()
        .user_agent(concat!("manga-sama/", env!("CARGO_PKG_VERSION")))
        // Long enough for a 3.6 MB page on a slow connection.
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .into()
});

/// A search result (`path` = slug) or a scan version (`path` = "scan/vf").
pub struct Link {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chapter {
    pub number: u32,
    pub pages: u32,
}

static START: LazyLock<Instant> = LazyLock::new(Instant::now);

/// One line per request on stderr, only when it is redirected (`manga-sama 2> log`): the TUI owns the terminal.
fn log(line: &str) {
    if !std::io::stderr().is_terminal() {
        eprintln!("{:8.3}s {line}", START.elapsed().as_secs_f64());
    }
}

fn get(url: &str) -> Result<ureq::Body> {
    let sent = Instant::now();
    let response = AGENT.get(url).call();
    let ms = sent.elapsed().as_millis();
    match &response {
        Ok(response) => log(&format!("{} {ms:>4} ms {url}", response.status().as_u16())),
        Err(e) => log(&format!("ERR {ms:>4} ms {url} : {e}")),
    }
    Ok(response.map_err(|e| format!("{url} : {e}"))?.into_body())
}

fn encode(s: &str) -> String {
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

pub fn search(query: &str) -> Result<Vec<Link>> {
    let url = format!(
        "{BASE}/catalogue/?search={}&type%5B%5D=Scans",
        encode(query)
    );
    Ok(parse_search(&get(&url)?.read_to_string()?))
}

pub fn versions(slug: &str) -> Result<Vec<Link>> {
    let url = format!("{BASE}/catalogue/{slug}/");
    Ok(parse_versions(&get(&url)?.read_to_string()?))
}

/// The work's name as the chapter API expects it.
pub fn title(slug: &str, path: &str) -> Result<String> {
    let url = format!("{BASE}/catalogue/{slug}/{path}/");
    parse_title(&get(&url)?.read_to_string()?)
        .ok_or_else(|| format!("titre introuvable sur {url}").into())
}

pub fn chapters(title: &str) -> Result<Vec<Chapter>> {
    let url = format!(
        "{BASE}/s2/scans/get_nb_chap_et_img.php?oeuvre={}",
        encode(title)
    );
    parse_chapters(&get(&url)?.read_to_string()?)
}

pub fn page(title: &str, chapter: u32, page: u32) -> Result<Vec<u8>> {
    let url = format!("{BASE}/s2/scans/{}/{chapter}/{page}.jpg", encode(title));
    Ok(get(&url)?.read_to_vec()?)
}

fn parse_search(html: &str) -> Vec<Link> {
    let doc = Html::parse_document(html);
    let link = Selector::parse(r#".catalog-card a[href*="/catalogue/"]"#).unwrap();
    let name = Selector::parse(".card-title").unwrap();
    doc.select(&link)
        .filter_map(|a| {
            let slug = a
                .attr("href")?
                .split("/catalogue/")
                .nth(1)?
                .trim_end_matches('/');
            let name = a.select(&name).next()?.text().collect::<String>();
            (!slug.is_empty()).then(|| Link {
                name: name.trim().to_string(),
                path: slug.to_string(),
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
            })
        })
        .filter(|v| (v.name.as_str(), v.path.as_str()) != ("nom", "url"))
        .collect()
}

// Not trimmed and HTML-escaped on purpose: the site sends `innerHTML` as is, and
// "Jujutsu Kaisen Modulo " only exists with its trailing space.
fn parse_title(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    let title = Selector::parse("#titreOeuvre").unwrap();
    doc.select(&title).next().map(|e| e.inner_html())
}

/// Empty chapters are dropped: the reader could never show them.
fn parse_chapters(json: &str) -> Result<Vec<Chapter>> {
    let chapters: Vec<_> = serde_json::from_str::<BTreeMap<u32, u32>>(json)
        .unwrap_or_default()
        .into_iter()
        .filter(|&(_, pages)| pages > 0)
        .map(|(number, pages)| Chapter { number, pages })
        .collect();
    if chapters.is_empty() {
        return Err(format!("liste des chapitres invalide : {json}").into());
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
                <img alt="Berserk"><h2 class="card-title">Berserk</h2></a></div>
            <div class="catalog-card"><a href="https://anime-sama.to/catalogue/one-piece/">
                <h2 class="card-title"> One Piece </h2></a></div>
            <a href="https://anime-sama.to/catalogue/">Catalogue</a>
        </div>"#;
        let works = parse_search(html);
        let got: Vec<_> = works
            .iter()
            .map(|w| (w.name.as_str(), w.path.as_str()))
            .collect();
        assert_eq!(got, [("Berserk", "berserk"), ("One Piece", "one-piece")]);
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
        assert_eq!(parse_title("<h3>rien</h3>"), None);
    }

    #[test]
    fn chapter_list() {
        let chapters = parse_chapters(r#"{"1":57,"2":25,"10":3}"#).unwrap();
        let expected = [(1, 57), (2, 25), (10, 3)].map(|(number, pages)| Chapter { number, pages });
        assert_eq!(chapters, expected);
        assert_eq!(
            parse_chapters(r#"{"1":5,"2":0}"#).unwrap(),
            [Chapter {
                number: 1,
                pages: 5
            }]
        );
        assert!(parse_chapters(r#"{"error":"Oeuvre 'x' not found"}"#).is_err());
        assert!(parse_chapters("{}").is_err());
    }

    #[test]
    #[ignore = "réseau"]
    fn full_walk() {
        let work = search("berserk")
            .unwrap()
            .into_iter()
            .find(|w| w.path == "berserk")
            .expect("berserk trouvé");
        let version = versions(&work.path)
            .unwrap()
            .into_iter()
            .next()
            .expect("une version");
        let title = title(&work.path, &version.path).unwrap();
        let chapters = chapters(&title).unwrap();
        let bytes = page(&title, chapters[0].number, 1).unwrap();
        image::load_from_memory(&bytes).unwrap();
    }
}
