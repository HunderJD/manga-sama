//! UI texts, from the JSON files of `locales/` built into the binary. Ctrl+L switches language.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::LazyLock;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;

/// Built-in languages, in the order Ctrl+L cycles through them; the first is the default.
const LOCALES: [&str; 2] = [
    include_str!("../locales/en.json"),
    include_str!("../locales/fr.json"),
];

static TEXTS: LazyLock<Vec<HashMap<String, String>>> = LazyLock::new(|| {
    let parse = |json| serde_json::from_str(json).expect("locales are checked by a test");
    LOCALES.map(parse).into()
});
static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// Switches the UI to the next built-in language.
pub fn next_language() {
    CURRENT.store((CURRENT.load(Relaxed) + 1) % LOCALES.len(), Relaxed);
}

/// The text for `key`, or the key itself if it is missing.
pub fn t(key: &'static str) -> &'static str {
    TEXTS[CURRENT.load(Relaxed)]
        .get(key)
        .map_or(key, String::as_str)
}

/// `t` with its `{name}` placeholders filled.
pub fn tf(key: &'static str, args: &[(&str, &dyn Display)]) -> String {
    args.iter().fold(t(key).to_string(), |text, (name, value)| {
        text.replace(&format!("{{{name}}}"), &value.to_string())
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    #[test]
    fn locales_have_the_same_keys() {
        let keys = |json| {
            let texts: BTreeMap<String, String> = serde_json::from_str(json).unwrap();
            texts.into_keys().collect::<Vec<_>>()
        };
        for json in super::LOCALES {
            assert_eq!(keys(json), keys(super::LOCALES[0]));
        }
    }
}
