//! UI texts from `locales/<language>.json`, picked from the system language, English by default.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::LazyLock;

const LOCALES: [(&str, &str); 2] = [
    ("en", include_str!("../locales/en.json")),
    ("fr", include_str!("../locales/fr.json")),
];

static TEXTS: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    let vars =
        ["LC_ALL", "LC_MESSAGES", "LANG"].map(|name| std::env::var(name).unwrap_or_default());
    let language = language(&vars);
    let (_, json) = LOCALES
        .iter()
        .find(|(code, _)| *code == language)
        .unwrap_or(&LOCALES[0]);
    serde_json::from_str(json).expect("locales are checked by a test")
});

/// The first locale variable that is set, cut to its language: `fr_CA.UTF-8` → `fr`.
fn language(vars: &[String]) -> &str {
    vars.iter()
        .find(|var| !var.is_empty())
        .map_or("en", |var| var.get(..2).unwrap_or(var))
}

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
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn locales_have_the_same_keys() {
        let keys = |json| {
            let texts: BTreeMap<String, String> = serde_json::from_str(json).unwrap();
            texts.into_keys().collect::<Vec<_>>()
        };
        assert_eq!(keys(LOCALES[0].1), keys(LOCALES[1].1));
    }

    #[test]
    fn system_language() {
        let vars = |vars: [&str; 3]| vars.map(String::from);
        assert_eq!(language(&vars(["", "", "fr_FR.UTF-8"])), "fr");
        assert_eq!(language(&vars(["", "", "en_US.UTF-8"])), "en");
        assert_eq!(language(&vars(["", "fr_CA.UTF-8", "en_US.UTF-8"])), "fr");
        assert_eq!(language(&vars(["", "", ""])), "en");
    }
}
