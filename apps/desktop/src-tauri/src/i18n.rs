//! Übersetzungen für Texte, die Rust selbst anzeigt (Tray, Dialoge).
//! Dieselben Dateien wie im Frontend – eine Quelle für alle Texte.

use std::collections::HashMap;
use std::sync::OnceLock;

static DE: &str = include_str!("../../src/lib/i18n/de.json");
static EN: &str = include_str!("../../src/lib/i18n/en.json");

fn messages() -> &'static HashMap<String, String> {
    static MESSAGES: OnceLock<HashMap<String, String>> = OnceLock::new();
    MESSAGES.get_or_init(|| {
        let german = sys_locale::get_locale().is_some_and(|l| l.to_lowercase().starts_with("de"));
        let mut map: HashMap<String, String> =
            serde_json::from_str(EN).expect("en.json ist gültig");
        if german {
            let de: HashMap<String, String> = serde_json::from_str(DE).expect("de.json ist gültig");
            map.extend(de);
        }
        map
    })
}

/// Text zum Schlüssel; `{name}` wird durch den passenden Parameter ersetzt.
pub fn t(key: &str, params: &[(&str, &str)]) -> String {
    let mut text = messages().get(key).cloned().unwrap_or_else(|| key.into());
    for (name, value) in params {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn de_und_en_haben_dieselben_schluessel() {
        let de: HashMap<String, String> = serde_json::from_str(DE).unwrap();
        let en: HashMap<String, String> = serde_json::from_str(EN).unwrap();
        let mut a: Vec<_> = de.keys().collect();
        let mut b: Vec<_> = en.keys().collect();
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }

    #[test]
    fn platzhalter_werden_ersetzt() {
        assert!(t("error.capture", &[("error", "XY")]).contains("XY"));
    }
}
