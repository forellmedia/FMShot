//! Übersetzungen für Texte, die Rust selbst anzeigt (Tray, Dialoge).
//! Dieselben Dateien wie im Frontend – eine Quelle für alle Texte.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

static DE: &str = include_str!("../../src/lib/i18n/de.json");
static EN: &str = include_str!("../../src/lib/i18n/en.json");

fn table(german: bool) -> &'static HashMap<String, String> {
    static EN_MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    static DE_MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    let en = EN_MAP.get_or_init(|| serde_json::from_str(EN).expect("en.json ist gültig"));
    if !german {
        return en;
    }
    DE_MAP.get_or_init(|| {
        let mut map = en.clone();
        let de: HashMap<String, String> = serde_json::from_str(DE).expect("de.json ist gültig");
        map.extend(de);
        map
    })
}

/// Sprache aus den Einstellungen (`None` = System).
static LANGUAGE: RwLock<Option<String>> = RwLock::new(None);

pub fn set_language(lang: Option<&str>) {
    *LANGUAGE.write().unwrap_or_else(|e| e.into_inner()) = lang.map(str::to_string);
}

fn german() -> bool {
    match LANGUAGE
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .as_deref()
    {
        Some(l) => l.starts_with("de"),
        None => sys_locale::get_locale().is_some_and(|l| l.to_lowercase().starts_with("de")),
    }
}

/// Text zum Schlüssel; `{name}` wird durch den passenden Parameter ersetzt.
pub fn t(key: &str, params: &[(&str, &str)]) -> String {
    let mut text = table(german())
        .get(key)
        .cloned()
        .unwrap_or_else(|| key.into());
    for (name, value) in params {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

/// Text zum Schlüssel in allen Sprachen (z. B. für die Suche nach
/// Stichwörtern, egal in welcher Sprache getippt).
pub fn all(key: &str) -> Vec<String> {
    [table(false), table(true)]
        .iter()
        .filter_map(|t| t.get(key).cloned())
        .collect()
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
