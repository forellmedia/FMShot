//! Tastenkombinationen: Aktionen, Standardbelegung, Text-Format für die
//! Einstellungen. Das Registrieren beim System folgt in Meilenstein 2.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Was eine Tastenkombination auslöst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    CaptureScreen,
    CaptureRegion,
    CaptureWindow,
    CaptureScroll,
    CaptureText,
    OpenGallery,
}

impl Action {
    pub const ALL: [Action; 6] = [
        Action::CaptureScreen,
        Action::CaptureRegion,
        Action::CaptureWindow,
        Action::CaptureScroll,
        Action::CaptureText,
        Action::OpenGallery,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    MacOs,
    Windows,
    Linux,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(target_os = "windows") {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }
}

/// Modifikatoren. `primary` ist ⌘ auf macOS und Ctrl sonst – so bleibt eine
/// gespeicherte Belegung zwischen Plattformen sinnvoll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub primary: bool,
    pub shift: bool,
    pub alt: bool,
    /// Ctrl auf macOS bzw. Win/Super sonst.
    pub secondary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Digit(u8),
    Letter(char),
    PrintScreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub modifiers: Modifiers,
    pub key: Key,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("leere Tastenkombination")]
    Empty,
    #[error("unbekannte Taste „{0}“")]
    UnknownKey(String),
}

const PRIMARY_SHIFT: Modifiers = Modifiers {
    primary: true,
    shift: true,
    alt: false,
    secondary: false,
};
const PRIMARY_ALT: Modifiers = Modifiers {
    primary: true,
    shift: false,
    alt: true,
    secondary: false,
};

/// Standardbelegung laut docs/produkt.md.
///
/// `replace_mac_system` – ob der Nutzer auf macOS die Systemkürzel ⌘⇧3/4/5
/// ersetzen will. Sonst werden ⌘⌥3/4/5 verwendet.
pub fn default_bindings(platform: Platform, replace_mac_system: bool) -> Vec<(Action, Shortcut)> {
    let digit = |m, d| Shortcut {
        modifiers: m,
        key: Key::Digit(d),
    };
    let taken = if platform == Platform::MacOs && !replace_mac_system {
        PRIMARY_ALT
    } else {
        PRIMARY_SHIFT
    };
    let mut bindings = vec![
        (Action::CaptureScreen, digit(PRIMARY_SHIFT, 1)),
        (Action::CaptureRegion, digit(PRIMARY_SHIFT, 2)),
        (Action::CaptureWindow, digit(taken, 3)),
        (Action::CaptureScroll, digit(taken, 4)),
        (Action::CaptureText, digit(taken, 5)),
        (
            Action::OpenGallery,
            Shortcut {
                modifiers: PRIMARY_SHIFT,
                key: Key::Letter('G'),
            },
        ),
    ];
    if platform != Platform::MacOs {
        bindings.push((
            Action::CaptureRegion,
            Shortcut {
                modifiers: Modifiers::default(),
                key: Key::PrintScreen,
            },
        ));
    }
    bindings
}

/// Gespeichertes Format: `Primary+Shift+2`, `Primary+Alt+G`, `PrintScreen`.
impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.modifiers;
        for (on, name) in [
            (m.primary, "Primary"),
            (m.secondary, "Secondary"),
            (m.alt, "Alt"),
            (m.shift, "Shift"),
        ] {
            if on {
                write!(f, "{name}+")?;
            }
        }
        match self.key {
            Key::Digit(d) => write!(f, "{d}"),
            Key::Letter(c) => write!(f, "{c}"),
            Key::PrintScreen => f.write_str("PrintScreen"),
        }
    }
}

impl FromStr for Shortcut {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let key = parts
            .pop()
            .filter(|k| !k.is_empty())
            .ok_or(ParseError::Empty)?;
        let mut modifiers = Modifiers::default();
        for p in parts {
            match p {
                "Primary" => modifiers.primary = true,
                "Secondary" => modifiers.secondary = true,
                "Alt" => modifiers.alt = true,
                "Shift" => modifiers.shift = true,
                other => return Err(ParseError::UnknownKey(other.into())),
            }
        }
        let key = match key {
            "PrintScreen" => Key::PrintScreen,
            k if k.len() == 1 => {
                let c = k.chars().next().expect("Länge 1 geprüft");
                if let Some(d) = c.to_digit(10) {
                    Key::Digit(d as u8)
                } else if c.is_ascii_alphabetic() {
                    Key::Letter(c.to_ascii_uppercase())
                } else {
                    return Err(ParseError::UnknownKey(k.into()));
                }
            }
            k => return Err(ParseError::UnknownKey(k.into())),
        };
        Ok(Shortcut { modifiers, key })
    }
}

impl Shortcut {
    /// Anzeige für Menschen: `⌘⇧2` auf macOS, `Ctrl+Shift+2` sonst.
    pub fn label(&self, platform: Platform) -> String {
        let m = self.modifiers;
        let key = match self.key {
            Key::Digit(d) => d.to_string(),
            Key::Letter(c) => c.to_string(),
            Key::PrintScreen => "PrintScreen".into(),
        };
        if platform == Platform::MacOs {
            let mut s = String::new();
            for (on, sym) in [
                (m.secondary, "⌃"),
                (m.alt, "⌥"),
                (m.shift, "⇧"),
                (m.primary, "⌘"),
            ] {
                if on {
                    s.push_str(sym);
                }
            }
            s + &key
        } else {
            let mut parts = Vec::new();
            for (on, name) in [
                (m.primary, "Ctrl"),
                (m.secondary, "Win"),
                (m.alt, "Alt"),
                (m.shift, "Shift"),
            ] {
                if on {
                    parts.push(name.to_string());
                }
            }
            parts.push(key);
            parts.join("+")
        }
    }
}

/// Doppelt belegte Kombinationen – für die Konfliktwarnung in den Einstellungen.
pub fn conflicts(bindings: &[(Action, Shortcut)]) -> Vec<(Action, Action, Shortcut)> {
    let mut found = Vec::new();
    for (i, (a, sa)) in bindings.iter().enumerate() {
        for (b, sb) in &bindings[i + 1..] {
            if sa == sb && a != b {
                found.push((*a, *b, *sa));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(bindings: &[(Action, Shortcut)], action: Action) -> Shortcut {
        bindings.iter().find(|(a, _)| *a == action).unwrap().1
    }

    #[test]
    fn mac_ohne_ersetzen_weicht_auf_alt_aus() {
        let b = default_bindings(Platform::MacOs, false);
        assert_eq!(
            find(&b, Action::CaptureRegion).label(Platform::MacOs),
            "⇧⌘2"
        );
        assert_eq!(
            find(&b, Action::CaptureWindow).label(Platform::MacOs),
            "⌥⌘3"
        );
    }

    #[test]
    fn mac_mit_ersetzen_nimmt_systemkuerzel() {
        let b = default_bindings(Platform::MacOs, true);
        assert_eq!(find(&b, Action::CaptureText).label(Platform::MacOs), "⇧⌘5");
    }

    #[test]
    fn windows_hat_druck_fuer_bereich() {
        let b = default_bindings(Platform::Windows, false);
        let region: Vec<_> = b
            .iter()
            .filter(|(a, _)| *a == Action::CaptureRegion)
            .map(|(_, s)| s.label(Platform::Windows))
            .collect();
        assert_eq!(region, ["Ctrl+Shift+2", "PrintScreen"]);
    }

    #[test]
    fn standardbelegung_ohne_konflikte() {
        for p in [Platform::MacOs, Platform::Windows, Platform::Linux] {
            for replace in [false, true] {
                assert!(conflicts(&default_bindings(p, replace)).is_empty());
            }
        }
    }

    #[test]
    fn text_format_hin_und_zurueck() {
        for (_, s) in default_bindings(Platform::Linux, false) {
            assert_eq!(s.to_string().parse::<Shortcut>().unwrap(), s);
        }
        assert_eq!(
            "Primary+Shift+x".parse::<Shortcut>().unwrap().key,
            Key::Letter('X')
        );
        assert!("Hyper+1".parse::<Shortcut>().is_err());
        assert_eq!("".parse::<Shortcut>(), Err(ParseError::Empty));
    }

    #[test]
    fn konflikt_wird_erkannt() {
        let s: Shortcut = "Primary+Shift+1".parse().unwrap();
        let c = conflicts(&[(Action::CaptureScreen, s), (Action::OpenGallery, s)]);
        assert_eq!(c.len(), 1);
    }
}
