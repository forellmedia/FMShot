# Testanleitung

Stand: 17. Oktober 2026. Alles ist für **macOS** umgesetzt und getestet.
**Windows** ist umgesetzt und baut in der CI, muss aber noch von Hand in der
VM geprüft werden ([Abschnitt Windows](#windows-utm-vm-windows-11-arm)).
Linux baut in der CI, nimmt aber noch nicht auf (siehe „Noch offen“).

# macOS

## Vorbereitung

```bash
cd ~/04_Software/Nabshot/apps/desktop
npm install
npm run tauri build -- --bundles app
open ../../target/release/bundle/macos/Nabshot.app
```

Nabshot erscheint als Symbol in der Menüleiste (kein Dock-Symbol, solange der
Editor zu ist). Beim ersten Aufnehmen fragt macOS nach **Bildschirmaufnahme** –
erlauben und Nabshot über das Menüleisten-Symbol → *Nabshot beenden* schliessen
und neu öffnen.

> Solange das eigene Zertifikat nicht eingerichtet ist
> (`./scripts/make-signing-cert.sh`), fragt macOS nach **jedem neuen Build**
> erneut nach der Freigabe.

## Tastenkürzel

| Aktion            | Kürzel |
|-------------------|--------|
| Bereich           | ⇧⌘2    |
| Vollbild          | ⇧⌘1    |
| Fenster           | ⌥⌘3    |
| Scroll-Aufnahme   | ⌥⌘4 (nochmals drücken = Fertig) |
| Text erfassen     | ⌥⌘5    |
| Video             | ⇧⌘6 (nochmals drücken = Stopp) |
| Galerie / Editor  | ⌥⌘G    |
| Einstellungen     | ⌘, (im Editor) |

Klick aufs Menüleisten-Symbol öffnet das Tray-Fenster (Tabs Bild, Video,
Einstellungen), Rechtsklick das klassische Menü.

## Checkliste

### 1. Bereich (⇧⌘2)
- [ ] Overlay erscheint sofort; **Fadenkreuz über den ganzen Bildschirm**
- [ ] Lupe neben dem Zeiger, Pixelmasse beim Ziehen
- [ ] Esc oder Rechtsklick bricht ab
- [ ] Nach dem Loslassen: **Vorschau an der Maus** (Kopieren · Speichern ·
      Bearbeiten · Text), Bild liegt schon in der Zwischenablage
- [ ] Vorschau-Bild in Mail/Slack/Finder **ziehen**
- [ ] Klick aufs Vorschau-Bild öffnet den Editor
- [ ] Einfügen in Vorschau/Keynote: Retina-Bild in **normaler Grösse**

### 2. Fenster (⌥⌘3) und Vollbild (⇧⌘1)
- [ ] Fenster unter der Maus wird blau hervorgehoben, Klick nimmt es auf
      (runde Ecken durchsichtig)
- [ ] Vollbild nimmt den Monitor auf, auf dem die Maus ist

### 3. Text erfassen (⌥⌘5)
- [ ] Bereich mit Text wählen → Hinweis „Text kopiert (n Zeichen)“
- [ ] Umlaute stimmen

### 4. Scroll-Aufnahme (⌥⌘4)
- [ ] Bereich wählen (z. B. Inhalt einer Webseite), dann langsam scrollen
- [ ] Leiste zeigt die Höhe; „Fertig“ oder erneut ⌥⌘4 beendet
- [ ] Ergebnis öffnet im Editor, ohne doppelte Zeilen
- [ ] Einstellungen → Allgemein → Scroll-Aufnahme „Nabshot scrollt
      automatisch“: fragt einmal nach **Bedienungshilfen**, scrollt dann selbst
- [ ] Bedienungshilfen ausgeschaltet → erneuter Versuch öffnet **direkt**
      Systemeinstellungen → Bedienungshilfen (kein eigenes Nabshot-Fenster)

### 4a. Tray-Fenster
- [ ] Klick aufs Menüleisten-Symbol: Fenster erscheint direkt darunter
- [ ] Klick daneben oder nochmals aufs Symbol schliesst es
- [ ] Rechtsklick aufs Symbol: klassisches Menü (inkl. „Video aufnehmen“)
- [ ] Tab **Bild**: jede Aufnahmeart startet, das Fenster ist dabei weg
- [ ] Tab **Einstellungen**: „Beim Anmelden starten“ an → abmelden/anmelden
      → Nabshot läuft; „Alle Einstellungen öffnen …“ zeigt die Einstellungen

### 4b. Video (⇧⌘6, ADR 0011 Etappe 1)
- [ ] Tab **Bildschirm**: Bereich wählen, Mikrofon an → beim ersten Mal fragt
      macOS nach dem Mikrofon
- [ ] Countdown 3–2–1 in der Leiste, roter Rahmen um den Bereich
- [ ] Leiste zeigt grosse Zeit mit rotem Punkt und daneben den Mikrofon-Pegel
      (sprechen → Balken); in der Pause ist der Punkt grau
- [ ] Pause → Zeit steht, „Weiter“ → läuft weiter; im Video fehlt die Pause
- [ ] Stopp (Leiste oder ⇧⌘6) → Vorschau erscheint, Video liegt in der
      Zwischenablage (in Finder/Mail einfügen)
- [ ] Galerie: Kachel mit ▶ und Dauer; öffnen spielt das Video mit Ton
      (Mikrofon und Ton des Computers, z. B. YouTube im Hintergrund)
- [ ] Leiste und Rahmen sind im Video nicht zu sehen
- [ ] Ziel **Fenster**: Fenster anklicken, Fenster verschieben → Video und
      roter Rahmen folgen
- [ ] Ziel **Vollbild**: ganzer Monitor unter der Maus, roter Rahmen am
      Monitorrand, Leiste unten
- [ ] „Abbrechen“ → kein Eintrag in der Galerie

### 4c. Absturzsicher (ADR 0012)
- [ ] Aufnahme 20 s mit Ton und Webcam, dann `kill -9 $(pgrep -x nabshot-desktop)`
      → Nabshot starten → Frage „Abgebrochene Aufnahme gefunden“ →
      „Wiederherstellen“ → Hinweis, Video in der Galerie, mit Ton, ~18–20 s
- [ ] Dasselbe, aber „Später“ → beim nächsten Start wird wieder gefragt;
      „Verwerfen“ → weg, keine Reste im Bibliotheksordner
- [ ] Fenster aufnehmen und das Fenster schliessen → Rahmen und Text orange
      („Unterbrochen – gesichert bis …“), Video ist gesichert, danach Hinweis
- [ ] Während der Aufnahme über Menü/Tray „Nabshot beenden“ → Video wird
      gesichert, danach beendet sich Nabshot
- [ ] Mikrofon in den Systemeinstellungen verbieten → Start öffnet direkt
      Systemeinstellungen → Mikrofon

### 4c. Webcam (ADR 0011 Etappe 2)
- [ ] Tab **Bildschirm**: Schalter **Webcam** an, Kamera wählen (z. B.
      FaceTime-Kamera oder iPhone); ohne Kamera am Mac ist der Schalter grau
- [ ] Beim ersten Start fragt macOS nach der **Kamera** – erlauben
- [ ] Während des Countdowns erscheint die **Blase** (rund, weisser Rahmen,
      Schatten) unten rechts im aufgenommenen Bereich, gespiegelt wie ein
      Spiegel
- [ ] Blase mit der Maus verschieben; Scrollrad/Trackpad über der Blase
      ändert die Grösse
- [ ] Die Blase ist im Bildschirmvideo nicht doppelt zu sehen: Im fertigen
      Video liegt die Webcam genau dort, wo die Blase zuletzt war (nahe einer
      Ecke rastet sie in der Ecke ein)
- [ ] Leiste: **Webcam aus** → Blase verschwindet, im Video blendet die
      Webcam weich aus; **Webcam an** → kommt nach ca. 1 s wieder
- [ ] Leiste: **Gross** → im Video füllt die Webcam das Bild, der Bildschirm
      liegt klein in der Ecke; **Klein** → zurück (weicher Übergang)
- [ ] Webcam im Tab aus, während der Aufnahme mit **Webcam an** zuschalten
      → Webcam erscheint im Video ab diesem Moment
- [ ] Formen im Tab: Quadrat (mit/ohne **Ecken abrunden**), 16:9,
      Hochformat; **Rahmen**, **Schatten**, **Spiegeln** aus → Blase und
      Video folgen
- [ ] Grösse Klein/Mittel/Gross → Blase und Webcam im Video entsprechend
- [ ] Pause mit Webcam: im Video fehlt die Pause auch bei der Webcam,
      Ton und Lippen bleiben synchron
- [ ] Nach dem Stopp zeigt die Leiste „Video wird erstellt … n %“, dann
      erscheint die Vorschau; Messwerte (Info → Messwerte) nennen
      „zusammengesetzt … (Apple …)“
- [ ] Im Monatsordner liegen `<id>_screen.mp4`, `<id>_webcam.mp4`,
      `<id>_video.mp4` und `<id>.json` mit `"video": { … "webcam": … }`
- [ ] Kamera in den Systemeinstellungen verbieten → Start mit Webcam öffnet
      direkt Systemeinstellungen → Kamera (kein eigenes Hinweisfenster)

### 4c+. Kopf und Hintergrund (ADR 0011 Etappe 3)
- [ ] Tab **Bildschirm** → Webcam: **Ausschnitt** = **Kopf** → während des
      Countdowns zeigt die Blase nach einem Moment nur Kopf und Schultern
- [ ] Mit **Kopf folgen** zur Seite gehen → der Ausschnitt zieht nach ca.
      1 s weich nach, kleines Wackeln bewegt ihn nicht; ohne **Kopf folgen**
      bleibt er nach ca. 2 s stehen
- [ ] Im fertigen Video folgt der Ausschnitt dem Kopf genauso; im
      Monatsordner liegt `<id>_faces.json`
- [ ] Kurz aus dem Bild gehen → Ausschnitt bleibt stehen, kein Springen
- [ ] **Hintergrund** = **Unscharf** → Blase und Video: Person scharf,
      Hintergrund weich
- [ ] **Hintergrund** = **Entfernen** → in der Blase und im Video schwebt
      nur die Person über dem Bildschirm (ohne Rahmen und Schatten; die
      Schalter dafür verschwinden im Tab)
- [ ] **Entfernen** + Leiste **Gross** → hinter der Person eine dunkle
      Fläche, der Bildschirm klein in der Ecke
- [ ] Kopf und Hintergrund zusammen, **Spiegeln** an/aus → Blase und Video
      gleich gespiegelt
- [ ] Mit **Ganzes Bild** + **Original** zeigt die Blase wie bisher das
      Kamerabild ohne Verzögerung
- [ ] Prozessorlast während der Aufnahme (Aktivitätsanzeige) mit Kopf und
      Hintergrund deutlich unter 1 Kern

### 4c++. Nur Webcam
- [ ] Tray-Fenster: Tabs **Bild · Bildschirm · Webcam · Einstellungen**
- [ ] Tab **Webcam** → **Hoch 9:16** → **Webcam-Aufnahme starten** (oder
      ⌘⇧7): Blase im Hochformat mitten auf dem Bildschirm, Countdown in
      der Leiste, keine Webcam-Knöpfe in der Leiste
- [ ] Keine Abfrage für die Bildschirmaufnahme
- [ ] Nochmals ⌘⇧7 = Stopp → fertiges Video 1080×1920, nur die Webcam,
      mit Ton vom Mikrofon
- [ ] **Quer 16:9** und **Quadrat 1:1** ebenso (1920×1080 bzw. 1080×1080)
- [ ] **Kopf** mit **Kopf folgen** → Blase und Video folgen dem Kopf
- [ ] **Hintergrund unscharf** → Video mit weichem Hintergrund
- [ ] Im Editor: Video spielt ab, Ausschnitt wie im Video; keine Knöpfe
      für Webcam klein/gross und Zoom; Schneiden und Pfeile gehen
- [ ] Ohne Kamera am Mac: Tab zeigt „Keine Kamera gefunden.“

### 4c+++. Einstellungen für das Video
- [ ] Einstellungen → Tab **Video**: Auflösung, Codec, Qualität, Bilder
      pro Sekunde
- [ ] **720p** + **HEVC** + **Klein** → neues Video aufnehmen → im Finder
      (Informationen) 720 Pixel hoch, HEVC, deutlich kleinere Datei
- [ ] **Wie aufgenommen** + **H.264** + **Hoch** → volle Grösse, grössere
      Datei
- [ ] Bei 720p Pfeil im Editor zeichnen → im fertigen Video an derselben
      Stelle, scharf
- [ ] Altes Video öffnen, kleine Änderung → wird mit der neuen Einstellung
      neu zusammengesetzt

### 4c++++. Live zeichnen
- [ ] Bildschirm-Aufnahme starten → Leiste hat **Zeichnen**; ⌘⇧8 oder
      Klick → links Werkzeugleiste, Fadenkreuz
- [ ] Pfeil ziehen → erscheint sofort; nach 3 s ohne neue Zeichnung
      verschwindet er
- [ ] Mehrere Zeichnungen hintereinander → verschwinden gemeinsam 3 s nach
      der letzten
- [ ] Nummer (N) dreimal klicken → 1, 2, 3; warten bis weg → wieder 1
- [ ] Esc → alles weg; nochmals Esc → Zeichnen beendet, Klicks gehen wieder
      an die Apps darunter, Tippen geht an die App
- [ ] Tasten A/L/R/O/P/N/H wählen das Werkzeug; Farbe wechseln; Shift
- [ ] Textmarker: gelb, halbdurchsichtig
- [ ] Leiste und Webcam-Blase bleiben während des Zeichnens bedienbar
- [ ] Fertiges Video: Zeichnungen genau dort und so lange wie live; im
      Bildschirmvideo selbst (Editor: Ebenen ausblenden) nichts eingebrannt
- [ ] Editor: Zeichnungen sind Ebenen, lassen sich löschen und verschieben
- [ ] ⌘⇧9 pausiert und setzt fort
- [ ] Nur Webcam: kein Knopf **Zeichnen**

### 4d. Video-Editor, Zeiger, Ebenen (ADR 0011 Etappe 4–6)
- [ ] Video in der Galerie öffnen: Leinwand zeigt das Video, darunter die
      Zeitleiste; Leertaste spielt ab und hält an (mit Ton)
- [ ] Mauszeiger ist im Video zu sehen (Nabshot zeichnet ihn), Klicks als
      roter Kreis; Zeitleiste → Zeiger-Symbol: Grösse, Klicks aus
- [ ] Auf der Zeitleiste einen Bereich ziehen → **Ausschneiden**: Länge wird
      kürzer, roter Strich zeigt den Schnitt; Abspielen springt darüber
- [ ] **Nur Auswahl behalten** (Trimmen); **Schnitte zurücknehmen**
- [ ] Webcam: Bereich wählen → **Gross** / **Klein** / **Aus**
- [ ] **Zoom hinzufügen** an der Stelle; Zoom-Block anklicken → Stärke,
      Links/Rechts, Oben/Unten, löschen; **Auto-Zoom** aus den Klicks
- [ ] Pfeil oder Text zeichnen: erscheint ab der aktuellen Stelle für 4 s
      (orange Spur); **Ab hier** / **Bis hier** / **Immer zeigen**
- [ ] Ton-Symbol: Lautstärke Computer/Mikrofon
- [ ] Nach Änderungen „Video wird aktualisiert … %“ → danach Galerie-Video,
      Kopieren (⌘C), Sichern unter (⌘S) mit allen Änderungen
- [ ] **Einzelbild** landet als Bild in der Galerie; **GIF** sichern und in
      einem Browser öffnen
- [ ] ⌘Z nimmt Schnitte, Webcam-Wechsel, Zooms und Ebenen zurück

### 4e. Zeitleiste mit Zeilen je Werkzeug
- [ ] Video mit live gezeichneten Pfeilen, Nummern, Textmarker öffnen →
      je Werkzeug eine Zeile mit Namen links, Blöcke in der Farbe der
      Zeichnung, Nummern zeigen ihre Zahl
- [ ] Zwei Pfeile gleichzeitig → zweite Pfeil-Zeile
- [ ] Block anklicken → Ebene auf der Leinwand gewählt, Abspielkopf springt
      hin, falls nötig
- [ ] Block ziehen → verschoben; Rand ziehen → länger/kürzer; ⌘Z macht es
      in einem Schritt rückgängig
- [ ] **Löschen** unter der Zeitleiste (oder ⌫) → Block und Zeichnung weg;
      danach neu gezeichneter Pfeil erscheint ab der aktuellen Stelle 4 s
- [ ] Zeile **Vergrössern**: Maus darüber erklärt, was es tut
- [ ] Textmarker im Video-Editor halbdurchsichtig
- [ ] Fertiges Video folgt den geänderten Zeiten

### 5. Editor
- [ ] Werkzeuge: Pfeil (A), Linie (L), Rechteck (R), Ellipse (O),
      Freihand (P), Text (T), Sprechblase (B), Nummern (N), Marker (H),
      Spotlight (S), Lupe (M), Weichzeichnen (U), Verpixeln (X),
      Zuschneiden (C), Hintergrund (G), Auswählen (V)
- [ ] Griffe zum Ändern, Shift = gerade Linien / Quadrate
- [ ] Text direkt tippen, Umlaute, Esc beendet
- [ ] Emoji im Text (⌃⌘Leertaste): farbig, auch ❤️, 👍🏽, 🇨🇭; im Export gleich
- [ ] Drehen: runder Griff über der Ebene ziehen (⇧ = 15°-Schritte) oder
      Gradzahl oben eingeben; „Gerade“ setzt zurück; gedrehter Text bleibt
      bearbeitbar (Doppelklick); Griffe ändern die Grösse, die Gegenseite
      bleibt stehen
- [ ] Mehrfachauswahl: ⇧-Klick oder Rahmen aufziehen (Auswählen-Werkzeug),
      ⌘A; gemeinsam verschieben (Maus/Pfeiltasten), drehen, löschen
- [ ] Gruppen: ⌘G gruppiert, ⇧⌘G löst auf; Klick wählt die ganze Gruppe,
      ⌘-Klick eine einzelne Ebene darin; ⌘C/⌘V und ⌘D kopieren die Gruppe;
      Ebenen-Panel zeigt „Gruppe n“ mit Sichtbar/Sperre/Reihenfolge
- [ ] ⌘Z / ⇧⌘Z, ⌘C kopiert das Bild (mit Auswahl: die Ebene), ⌘D dupliziert
- [ ] Bild aus dem Finder hineinziehen, ⌘V fügt Bild aus der
      Zwischenablage ein, Galerie-Bild auf die Leinwand ziehen
- [ ] Bild-Ebene auswählen, Zuschneiden-Werkzeug → nur das Bild wird
      zugeschnitten
- [ ] Hintergrund (G): Verlauf wählen → Rand, runde Ecken, Schatten
- [ ] „Text kopieren“ (T-Symbol oben) markiert erkannte Zeilen, Klick
      kopiert eine Zeile
- [ ] „Auto-Schwärzen“ verpixelt E-Mail-Adressen, IBANs, Telefonnummern
- [ ] Ebenen-Panel: Sichtbarkeit, Sperre, Reihenfolge
- [ ] PNG / JPG exportieren, „Kopieren“
- [ ] Zoom: Pinch / ⌘-Mausrad, ⌘0 einpassen, ⌘1 = 100 %; Leertaste + Ziehen
      verschiebt

### 6. Galerie
- [ ] Neue Aufnahmen erscheinen unten; Rechtsklick: Stern, Im Finder
      zeigen, Löschen
- [ ] **Suche** findet Aufnahmen nach Text darin (z. B. ein Wort aus einer
      Fehlermeldung)
- [ ] Ablage: `~/Library/Application Support/Nabshot/Library/2026/10/`
      – alles flach im Monatsordner

### 7. Einstellungen (⌘, oder Menüleiste → Einstellungen …)
- [ ] Nach der Aufnahme: Vorschau / Editor / nur kopieren
- [ ] Tastenkürzel neu belegen (Feld anklicken, Kombination drücken)
- [ ] Auto-Export einschalten, Ordner wählen → jede Aufnahme landet dort,
      Änderungen im Editor aktualisieren die Datei
- [ ] Sprache Englisch → Oberfläche und Menüleiste wechseln
- [ ] Info → Messwerte (Taste → Overlay in ms), „Fehler melden …“ öffnet
      ein vorausgefülltes GitHub-Issue

# Windows (UTM-VM, Windows 11 ARM)

Was neu ist: Aufnahme über GDI, eigenes Overlay (randlose Fenster, die nie
den Fokus nehmen), Texterkennung mit Windows.Media.Ocr, Zwischenablage mit
PNG + DIB (inkl. DPI), Vorschau mit Drag & Drop. Bisher nur gebaut und mit
Unit-Tests geprüft – **noch nie auf einem echten Desktop gelaufen.**

## Vorbereitung (einmalig in der VM)

1. **Visual Studio 2022 Build Tools** mit „Desktopentwicklung mit C++“; unter
   „Einzelne Komponenten“ zusätzlich *MSVC … ARM64/ARM64EC-Buildtools*.
2. **Rust** über <https://rustup.rs> (wählt `aarch64-pc-windows-msvc`), dann
   ```bash
   rustup target add wasm32-unknown-unknown
   cargo install wasm-bindgen-cli --version 0.2.129
   ```
3. **Node.js 22** (ARM64) und **Git für Windows**. Alle Befehle unten in
   **Git Bash** ausführen (das WASM-Skript braucht `bash`).
4. Repository in der VM klonen (nicht im geteilten Ordner bauen – zu langsam).

```bash
cd ~/Nabshot/apps/desktop
npm install
npm run tauri build -- --bundles nsis
```

Installer: `target/release/bundle/nsis/Nabshot_0.1.0_arm64-setup.exe`.
Schneller zum Ausprobieren: `npm run tauri dev`.

Nabshot erscheint im Infobereich der Taskleiste (ggf. unter dem Pfeil ^).
Windows fragt nicht nach einer Freigabe für Bildschirmaufnahme.

> **Druck-Taste:** Windows 11 öffnet damit das Snipping Tool. Für Nabshot
> unter *Einstellungen → Barrierefreiheit → Tastatur* „Mit der Druck-Taste
> Bildschirmausschnitt öffnen“ ausschalten (danach Nabshot neu starten).

## Tastenkürzel

| Aktion            | Kürzel |
|-------------------|--------|
| Bereich           | Strg+Umschalt+2 oder Druck |
| Vollbild          | Strg+Umschalt+1 |
| Fenster           | Strg+Umschalt+3 |
| Scroll-Aufnahme   | Strg+Umschalt+4 (nochmals drücken = Fertig) |
| Text erfassen     | Strg+Umschalt+5 |
| Galerie / Editor  | Strg+Alt+G |

## Checkliste

### W1. Bereich (Strg+Umschalt+2)
- [ ] Overlay erscheint sofort: Bildschirm leicht abgedunkelt, eingefroren,
      **Fadenkreuz über den ganzen Monitor**
- [ ] Die App darunter **behält den Fokus** (Titelleiste bleibt aktiv,
      Taskleiste wechselt nicht)
- [ ] Lupe neben dem Zeiger (Pixel scharf, rotes Kästchen = Pixel unter der
      Maus), Pixelmasse beim Ziehen; Ziehen in jede Richtung
- [ ] Nur klicken (ohne Ziehen) lässt das Overlay offen
- [ ] **Esc** und **Rechtsklick** brechen ab; danach funktioniert Esc in
      anderen Programmen wieder normal
- [ ] Ziehen ruckelt nicht (auch bei grossen Bereichen)
- [ ] Nach dem Loslassen: **Vorschau an der Maus** (Kopieren · Speichern ·
      Bearbeiten · Text), nimmt keinen Fokus, verschwindet nach ~6 s (bleibt,
      solange die Maus darauf ist)
- [ ] Vorschau-Bild in den **Explorer**, in Outlook/Mail oder einen Browser
      ziehen → Datei kommt an (wird kopiert, nicht verschoben)
- [ ] Klick aufs Vorschau-Bild öffnet den Editor; ✕ schliesst

### W2. Zwischenablage
- [ ] Einfügen in **Paint** (Bild vollständig, Farben stimmen)
- [ ] Einfügen in **Word**/WordPad/Outlook: bei *Anzeige → Skalierung 150 %*
      in **normaler Grösse**, nicht 1,5-fach
- [ ] „Text erfassen“ → Einfügen im Editor: Zeilenumbrüche stimmen

### W3. Fenster (Strg+Umschalt+3) und Vollbild (Strg+Umschalt+1)
- [ ] Bildschirm wird abgedunkelt, Fenster unter der Maus **blau
      hervorgehoben** (laufender Bildschirm, nicht eingefroren)
- [ ] Klick nimmt das Fenster auf – auch wenn ein anderes Fenster es teilweise
      verdeckt; runde Ecken (Windows 11) durchsichtig; kein Schatten/Rand
- [ ] Browser-Fenster (Edge) ist nicht schwarz
- [ ] Vollbild nimmt den Monitor auf, auf dem die Maus ist
- [ ] Ein offener Nabshot-Editor ist **nicht** in der Aufnahme zu sehen

### W4. Text erfassen (Strg+Umschalt+5)
- [ ] Bereich mit Text → Hinweis „Text kopiert (n Zeichen)“ unten in der Mitte
- [ ] Umlaute stimmen; auch ein einzelnes kleines Wort wird erkannt
- [ ] Fehlt die Texterkennung: Deutsch unter *Zeit und Sprache → Sprache*
      hinzufügen (oder PowerShell als Admin:
      `Add-WindowsCapability -Online -Name "Language.OCR~~~de-DE~0.0.1.0"`)

### W5. Scroll-Aufnahme (Strg+Umschalt+4)
- [ ] Roter Rahmen um den Bereich, Leiste darunter; Mausrad und Klicks gehen
      durch den Rahmen an die Seite
- [ ] Langsam scrollen → Leiste zeigt die Höhe; „Fertig“ oder erneut
      Strg+Umschalt+4 beendet; Ergebnis ohne doppelte Zeilen, ohne roten Rahmen
- [ ] Einstellung „Nabshot scrollt automatisch“: scrollt ohne Rückfrage selbst
      (Edge, Explorer, Einstellungen-App)

### W6. Skalierung und Monitore
- [ ] *Anzeige → Skalierung* auf 125 % / 150 %: Auswahl hat die richtige
      Pixelgrösse, Lupe/Schrift/Leisten sind entsprechend grösser
- [ ] Mit zwei Monitoren (falls die VM das kann): Overlay auf beiden,
      Fadenkreuz wechselt mit der Maus, Ziehen bleibt auf dem Start-Monitor

### W7. Rest
- [ ] Editor, Galerie und Einstellungen öffnen und zeigen die Aufnahmen
- [ ] Ablage: `%APPDATA%\Nabshot\Library\2026\10\`
- [ ] Info → Messwerte: „Taste → Overlay sichtbar“ unter 50 ms,
      „Taste → Bildschirm eingefroren“ notieren

# Noch offen
- **Linux:** Aufnahme, Overlay, Texterkennung, Vorschau und
  Zwischenablage-Feinheiten fehlen noch (App startet, Aufnahme meldet
  „noch nicht umgesetzt“).
- **Windows:** Bildstrom über Desktop Duplication / Windows Graphics Capture
  (erst mit der Video-Aufnahme nötig). Exportierte PNG-Dateien tragen 72 dpi
  × Skalierung (macOS-Basis) – unter Windows wären 96 dpi richtig.
- Signatur-/Updater-Secrets auf GitHub (Issue #2) – danach erster Release.
