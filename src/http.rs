//! The HTTP client every source goes through: one agent, an honest User-Agent, and a log line
//! per request.

use std::io::IsTerminal;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use ureq::Body;
use ureq::http::Response;

use crate::Result;

static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::Agent::config_builder()
        .user_agent(concat!("manga-sama/", env!("CARGO_PKG_VERSION")))
        // Long enough for a 3.6 MB page on a slow connection.
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .into()
});

/// Some pages weigh more than the 10 MB ureq accepts by default.
const MAX_IMAGE: u64 = 100 * 1024 * 1024;

static START: LazyLock<Instant> = LazyLock::new(Instant::now);

/// One line per request on stderr, only when it is redirected (`manga-sama 2> log`): the TUI owns the terminal.
fn log(line: &str) {
    if !std::io::stderr().is_terminal() {
        eprintln!("{:8.3}s {line}", START.elapsed().as_secs_f64());
    }
}

/// Logs a request sent at `sent` and hands back its body.
fn answered(
    url: &str,
    sent: Instant,
    response: Result<Response<Body>, ureq::Error>,
) -> Result<Body> {
    let ms = sent.elapsed().as_millis();
    match &response {
        Ok(response) => log(&format!("{} {ms:>4} ms {url}", response.status().as_u16())),
        Err(e) => log(&format!("ERR {ms:>4} ms {url} : {e}")),
    }
    Ok(response.map_err(|e| format!("{url} : {e}"))?.into_body())
}

pub fn text(url: &str) -> Result<String> {
    let sent = Instant::now();
    Ok(answered(url, sent, AGENT.get(url).call())?.read_to_string()?)
}

/// An image (a page or a cover).
pub fn bytes(url: &str) -> Result<Vec<u8>> {
    let sent = Instant::now();
    let body = answered(url, sent, AGENT.get(url).call())?;
    Ok(body.into_with_config().limit(MAX_IMAGE).read_to_vec()?)
}

/// A form sent with POST, like a browser does (a WordPress search, for example).
pub fn post_form(url: &str, form: &[(&str, &str)]) -> Result<String> {
    let sent = Instant::now();
    let response = AGENT.post(url).send_form(form.iter().copied());
    Ok(answered(url, sent, response)?.read_to_string()?)
}

/// Percent-encodes `s` for a URL query or path segment.
pub fn encode(s: &str) -> String {
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}
