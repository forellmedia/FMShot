//! Bildschirmfoto-Kürzel des Betriebssystems: Sind sie noch aktiv, kommen
//! FMShots Kürzel ⇧⌘3/4/5 nie an. Nur macOS kennt solche Kürzel.

/// macOS-Kürzel in `com.apple.symbolichotkeys`: 28 = ⇧⌘3, 30 = ⇧⌘4,
/// 184 = ⇧⌘5 (Bildschirmfoto-Optionen).
#[cfg(target_os = "macos")]
const SCREENSHOT_KEYS: [&str; 3] = ["28", "30", "184"];

/// Ob noch eines der Bildschirmfoto-Kürzel des Systems eingeschaltet ist.
/// `None`, wenn es auf dieser Plattform keine gibt oder sie sich nicht
/// lesen lassen.
pub fn screenshot_shortcuts_active() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        // `defaults` liefert den aktuellen Stand (auch wenn die Datei noch
        // nicht geschrieben ist).
        let out = std::process::Command::new("/usr/bin/defaults")
            .args(["export", "com.apple.symbolichotkeys", "-"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let root = plist::Value::from_reader_xml(out.stdout.as_slice()).ok()?;
        let keys = root
            .as_dictionary()?
            .get("AppleSymbolicHotKeys")
            .and_then(plist::Value::as_dictionary);
        Some(any_enabled(keys))
    }
    #[cfg(not(target_os = "macos"))]
    None
}

/// Fehlt ein Eintrag, gilt die Voreinstellung: eingeschaltet.
#[cfg(target_os = "macos")]
fn any_enabled(keys: Option<&plist::Dictionary>) -> bool {
    SCREENSHOT_KEYS.iter().any(|k| {
        keys.and_then(|d| d.get(k))
            .and_then(plist::Value::as_dictionary)
            .and_then(|d| d.get("enabled"))
            .and_then(plist::Value::as_boolean)
            .unwrap_or(true)
    })
}

/// Wo man die Kürzel des Systems ausschaltet (zum Öffnen im Browser/System).
pub fn shortcut_settings_url() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("x-apple.systempreferences:com.apple.Keyboard-Settings.extension")
    } else {
        None
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    fn dict(xml: &str) -> plist::Dictionary {
        plist::Value::from_reader_xml(xml.as_bytes())
            .unwrap()
            .into_dictionary()
            .unwrap()
    }

    #[test]
    fn fehlende_eintraege_gelten_als_eingeschaltet() {
        assert!(any_enabled(None));
        let aus = dict(
            r#"<?xml version="1.0"?><plist version="1.0"><dict>
            <key>28</key><dict><key>enabled</key><false/></dict>
            <key>30</key><dict><key>enabled</key><false/></dict>
            <key>184</key><dict><key>enabled</key><false/></dict>
            </dict></plist>"#,
        );
        assert!(!any_enabled(Some(&aus)));
        let teils = dict(
            r#"<?xml version="1.0"?><plist version="1.0"><dict>
            <key>28</key><dict><key>enabled</key><false/></dict>
            </dict></plist>"#,
        );
        assert!(any_enabled(Some(&teils)));
    }
}
