# 0013 – Neuer Name: Grabmark

**Status:** ersetzt durch ADR 0015 (ändert ADR 0008)

## Kontext
„FMShot“ sagt nichts darüber, was das Programm tut, und ist schwer
auszusprechen. Gesucht war ein englischer, sprechender Name ohne Konflikt mit
bestehenden Produkten oder Marken.

Geprüft (Oktober 2026) wurden rund 120 Namen: Domains (.com, .app, .ch),
Apple App Store (macOS und iOS), crates.io, npm, GitHub, Websuche und das
Markenregister TMview (u. a. EU, Schweiz, USA, UK). Ausgeschieden sind unter
anderem ShotQuill (bestehendes Tool für App-Store-Screenshots), Capsmith
(US-Marke, Klasse 9), Markcap (Marken) und Shottrove (zu nah an Shottr,
einem bekannten Screenshot-Tool für macOS).

## Entscheidung
Das Programm heisst **Grabmark** – *grab* (aufnehmen, „screen grab“) und
*mark* (markieren). In TMview kein Treffer; grabmark.app und grabmark.ch frei,
grabmark.com geparkt.

Umbenannt wurde alles, was den Namen trägt: Crates (`grabmark-*`), App
(`Grabmark.app`, Prozess `grabmark-desktop`), Kommandozeile `grabmark`,
Oberflächentexte, Dokumentation, GitHub-Repo `forellmedia/Grabmark`
(GitHub leitet alte Adressen weiter) und das selbst erstellte
Signatur-Zertifikat „Grabmark Local Signing“ (`~/.grabmark-signing/`).

Bewusst **nicht** umbenannt:
- App-ID `ch.forellmedia.fmshot` – macOS knüpft Berechtigungen daran.
- Speicherschlüssel im Frontend (`fmshot.presets`, `fmshot.tray.tab`) – sonst
  gingen Stil-Vorlagen verloren.

Datenordner: Tauri leitet seine Ordner aus der App-ID ab. Grabmark nutzt
stattdessen eigene unter seinem Namen (`apps/desktop/src-tauri/src/paths.rs`):

| Ordner | macOS |
|---|---|
| Bibliothek, Einstellungen | `~/Library/Application Support/Grabmark` |
| Cache (Bildinhalt) | `~/Library/Caches/Grabmark` |
| Logs | `~/Library/Logs/Grabmark` |

Unter Windows `%APPDATA%\Grabmark`, unter Linux `~/.local/share/Grabmark` und
`~/.config/Grabmark`. Es gibt keinen automatischen Umzug aus den alten Ordnern –
ausser dem Entwickler hatte niemand das Programm installiert.

## Folgen
- Das neue Zertifikat ist eine neue Identität: macOS verlangt die Freigaben
  (Bildschirmaufnahme, Bedienungshilfen, Mikrofon, Kamera) einmal neu. Die
  alten Einträge „FMShot“ in den Systemeinstellungen können entfernt werden.
- Die GitHub-Secrets `MACOS_CERT_P12_BASE64` / `MACOS_CERT_PASSWORD` müssen das
  neue Zertifikat enthalten.
- Eine eingetragene Marke (IGE, Klassen 9 und 42) vor dem Verkauf anmelden.
