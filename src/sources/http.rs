//! What every source uses to talk to its website: one HTTP agent, a log line per request, and a
//! session cache for the pages that are asked again and again.

use std::collections::HashMap;
use std::io::IsTerminal;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::Result;

static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::Agent::config_builder()
        .user_agent(concat!("manga-sama/", env!("CARGO_PKG_VERSION")))
        // Long enough for the biggest pages (over 10 MB) on a slow connection.
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .into()
});

static START: LazyLock<Instant> = LazyLock::new(Instant::now);

/// One line per request on stderr, only when it is redirected (`manga-sama 2> log`): the TUI owns the terminal.
fn log(line: &str) {
    if !std::io::stderr().is_terminal() {
        eprintln!("{:8.3}s {line}", START.elapsed().as_secs_f64());
    }
}

/// Logs the request and turns an error into one that names the URL.
fn logged(
    url: &str,
    request: impl FnOnce() -> Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<ureq::Body> {
    let sent = Instant::now();
    let response = request();
    let ms = sent.elapsed().as_millis();
    match &response {
        Ok(response) => log(&format!("{} {ms:>4} ms {url}", response.status().as_u16())),
        Err(e) => log(&format!("ERR {ms:>4} ms {url}: {e}")),
    }
    Ok(response.map_err(|e| format!("{url}: {e}"))?.into_body())
}

pub fn get_text(url: &str) -> Result<String> {
    Ok(logged(url, || AGENT.get(url).call())?.read_to_string()?)
}

pub fn post_form(url: &str, form: &[(&str, &str)]) -> Result<String> {
    let form = form.iter().copied();
    Ok(logged(url, || AGENT.post(url).send_form(form))?.read_to_string()?)
}

/// Answers already fetched while the app runs (a work's page, its chapter list…), by URL: going
/// back and forth between screens never asks the site twice. Gone when the app quits.
static SESSION: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);

pub fn get_text_cached(url: &str) -> Result<String> {
    if let Some(text) = SESSION
        .lock()
        .ok()
        .and_then(|session| session.get(url).cloned())
    {
        return Ok(text);
    }
    let text = get_text(url)?;
    if let Ok(mut session) = SESSION.lock() {
        session.insert(url.to_string(), text.clone());
    }
    Ok(text)
}

/// Some pages weigh more than the 10 MB ureq accepts by default.
const MAX_IMAGE: u64 = 100 * 1024 * 1024;

pub fn image(url: &str, referer: Option<&str>) -> Result<Vec<u8>> {
    let mut request = AGENT.get(url);
    if let Some(referer) = referer {
        request = request.header("Referer", referer);
    }
    Ok(logged(url, || request.call())?
        .into_with_config()
        .limit(MAX_IMAGE)
        .read_to_vec()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_cache_answers_without_the_site() {
        let url = "https://example.invalid/never-fetched";
        SESSION
            .lock()
            .unwrap()
            .insert(url.to_string(), "cached".to_string());
        assert_eq!(get_text_cached(url).unwrap(), "cached");
    }
}
