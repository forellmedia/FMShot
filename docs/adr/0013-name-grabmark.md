# 0013 – Neuer Name: Grabmark

**Status:** angenommen (ändert ADR 0008)

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
- App-ID `ch.forellmedia.fmshot` – macOS knüpft Berechtigungen daran, und die
  Datenordner (Bibliothek, Einstellungen, Logs) hängen von ihr ab.
- Speicherschlüssel im Frontend (`fmshot.presets`, `fmshot.tray.tab`) – sonst
  gingen Stil-Vorlagen verloren.

Übernahme beim ersten Start:
- Ein LaunchAgent `FMShot.plist` („mit der Anmeldung starten“) wird durch einen
  für Grabmark ersetzt.
- Das alte Standardmuster für Auto-Export-Dateinamen „FMShot {date} {time}“
  wird zu „Grabmark {date} {time}“; eigene Muster bleiben.

## Folgen
- Das neue Zertifikat ist eine neue Identität: macOS verlangt die Freigaben
  (Bildschirmaufnahme, Bedienungshilfen, Mikrofon, Kamera) einmal neu. Die
  alten Einträge „FMShot“ in den Systemeinstellungen können entfernt werden.
- Die GitHub-Secrets `MACOS_CERT_P12_BASE64` / `MACOS_CERT_PASSWORD` müssen das
  neue Zertifikat enthalten.
- Eine eingetragene Marke (IGE, Klassen 9 und 42) vor dem Verkauf anmelden.
