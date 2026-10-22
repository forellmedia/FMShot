# 0015 – Neuer Name: Nabshot

**Status:** angenommen (ersetzt ADR 0013)

## Kontext
„Grabmark“ gefiel auf Dauer nicht. Gewünscht war ein kurzer englischer Name,
ursprünglich „Grabit“. Geprüft wurden im Oktober 2026 rund 100 Namen auf die
gleiche Weise wie in ADR 0013: Domains (.com, .app, .ch, .io), Apple App
Store (macOS und iOS), crates.io, npm, GitHub, Websuche und die Markenregister
TMview (EU, USA, UK u. a.) und Swissreg.

Ausgeschieden sind unter anderem:
- **Grabit** – Screenshot-App „GrabIt“ im Mac App Store seit 2011, weitere
  Screenshot-Tools gleichen Namens unter Windows und Linux.
- **Grabbie** – selbst frei, aber zu nah an der Wortmarke GRABBIT, die in der
  Schweiz (Nr. 761569), in Deutschland, der EU und den USA für Software
  (Klassen 9 und 42) eingetragen ist.
- **Snapify** – bekanntes Open-Source-Programm für Bildschirmvideos.
- **Zappit** – Domains belegt, Software-Marken in mehreren Ländern.
- Echte Wörter wie Magpie, Osprey, Glint oder Clasp – überall vergeben.

## Entscheidung
Das Programm heisst **Nabshot** – *to nab* (schnappen, „grab“) und *shot*
(Aufnahme). In TMview und Swissreg kein Eintrag, im App Store keine App,
nabshot.app und nabshot.ch frei, crates.io und npm frei; nabshot.com ist
privat registriert (ohne Webseite).

Umbenannt wurde alles, was den Namen trägt: Crates (`nabshot-*`), App
(`Nabshot.app`, Prozess `nabshot-desktop`), Kommandozeile `nabshot`,
WASM-Modul, Oberflächentexte, Dokumentation, Skripte, CI, das Signatur-
Zertifikat „Nabshot Local Signing“ (`~/.nabshot-signing/`) und das
GitHub-Repo `forellmedia/Nabshot` (GitHub leitet alte Adressen weiter, auch
die Update-Adresse älterer Versionen).

Bewusst **nicht** umbenannt:
- App-ID `ch.forellmedia.fmshot` – macOS knüpft Berechtigungen daran.
- Speicherschlüssel `fmshot.*` im Frontend (siehe ADR 0013).
- Der lokale Arbeitsordner des Entwicklers.

## Umzug
Anders als bei ADR 0013 gibt es jetzt echte Daten. Beim ersten Start benennt
`paths::migrate_old_name()` die Ordner um, bevor das Log startet:

| Alt | Neu |
|---|---|
| `~/Library/Application Support/Grabmark` | `…/Nabshot` |
| `~/Library/Caches/Grabmark` | `…/Nabshot` |
| `~/Library/Logs/Grabmark` | `…/Nabshot` |

Die Bibliothek speichert nur relative Pfade, ein Umbenennen genügt. Gibt es
den neuen Ordner schon, bleibt der alte liegen (Meldung im Log). Das alte
Startobjekt `~/Library/LaunchAgents/Grabmark.plist` wird entfernt und – war
„Mit Anmeldung starten“ an – unter neuem Namen angelegt. `build-local.sh`
entfernt `/Applications/Grabmark.app`.

## Folgen
- Das neue Zertifikat ist eine neue Identität: macOS verlangt die Freigaben
  (Bildschirmaufnahme, Bedienungshilfen, Mikrofon, Kamera) einmal neu. Die
  alten Einträge „Grabmark“ in den Systemeinstellungen können entfernt werden,
  ebenso das alte Zertifikat „Grabmark Local Signing“ im Schlüsselbund.
- Die GitHub-Secrets `MACOS_CERT_P12_BASE64` / `MACOS_CERT_PASSWORD` müssen
  das neue Zertifikat enthalten.
- Vor dem Verkauf: Marke beim IGE anmelden (Klassen 9 und 42) und eine
  Ähnlichkeitsrecherche machen lassen; nabshot.app und nabshot.ch sichern.
