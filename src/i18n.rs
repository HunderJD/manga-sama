//! UI texts, from a JSON file of `locales/`.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::LazyLock;

/// The UI language: to change it, point this at another file of `locales/` and rebuild.
static TEXTS: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../locales/en.json")).expect("locales are checked by a test")
});

/// The text for `key`, or the key itself if it is missing.
pub fn t(key: &'static str) -> &'static str {
    TEXTS.get(key).map_or(key, String::as_str)
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
        assert_eq!(
            keys(include_str!("../locales/en.json")),
            keys(include_str!("../locales/fr.json"))
        );
    }
}
