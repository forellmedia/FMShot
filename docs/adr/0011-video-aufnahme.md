# 0011 – Video-Aufnahme mit Ton und Webcam

**Status:** angenommen (ändert docs/produkt.md: Video ist nicht mehr „Nicht in v0.1“,
und CLAUDE.md Regel 2: neue Plattform-Crate `media`)

## Kontext
FMShot soll Bildschirmvideos aufnehmen können, mit Mikrofon, Systemton und
Webcam – mindestens so viel wie Snagit. Die Webcam soll in jeder Ecke stehen
können, rund, quadratisch oder im Querformat sein, mit oder ohne Rundung, als
Vollbild oder nur mit dem Kopf, der auf Wunsch verfolgt wird.

Zuerst nur Apple Silicon (ADR 0009), die Bausteine müssen aber auch unter
Windows und Linux funktionieren. FFmpeg und GStreamer scheiden aus, weil sie
LGPL/GPL sind (ADR 0008).

## Entscheidung

### Aufnahme: getrennte Spuren, nichts eingebrannt
Jede Quelle wird für sich gespeichert, alle mit derselben Systemuhr als
Zeitbasis. Das Video entsteht erst beim Export. So bleiben Lage, Form und
Verfolgung der Webcam, Zoom und Mauszeiger nachträglich änderbar
(wie `doc.json` bei Bildern, ADR 0004).

Flach im Monatsordner wie Bilder (ADR 0004, Schema 2):

```
Library/2026/10/
  <id>.json             Dokument: Zeitachse, Schnitte, Webcam-Layout, Zoom, Ebenen
  <id>_screen.mp4       Bildschirm (HEVC)
  <id>_system.m4a       Systemton (AAC)
  <id>_mic.m4a          Mikrofon (AAC)
  <id>_webcam.mp4       Webcam (HEVC) – Etappe 2
  <id>_input.json       Mausspur: Position, Klicks, Zeigerform – Etappe 5
  <id>_faces.json       Kopfposition der Webcam, ~10× pro Sekunde – Etappe 3
  <id>_video.mp4        alles zusammengeführt (H.264 + AAC), sofort abspielbar
  <id>_original.png     Standbild (Galerie, Texterkennung, Bildinhalt)
  <id>_thumb.webp
```

Der Index kennt Videos an `mode = video` und der Spalte `duration_ms`.

- Fragmentiertes MP4: Nach einem Absturz bleibt alles bis auf die letzten
  Sekunden erhalten.
- Kodiert wird mit der Hardware des Systems. Die Aufnahme braucht kaum
  Prozessorzeit.

### Bausteine je Plattform

| Teil                  | macOS (jetzt)                      | Windows                       | Linux                     |
|-----------------------|------------------------------------|-------------------------------|---------------------------|
| Bildschirm            | ScreenCaptureKit `SCStream`        | Windows.Graphics.Capture      | PipeWire (Portal)         |
| Systemton             | ScreenCaptureKit (Audio)           | WASAPI-Loopback               | PipeWire-Monitor          |
| Mikrofon              | AVFoundation `AVCaptureSession`    | WASAPI                        | PipeWire                  |
| Webcam                | AVFoundation `AVCaptureSession`    | Media Foundation              | V4L2 / PipeWire           |
| Kodieren / Dekodieren | AVAssetWriter / AVAssetReader      | Media Foundation              | openh264 (BSD) + Opus     |
| Mausspur              | `NSEvent` (Klicks), Zeigerbild     | Raw Input, `GetCursorInfo`    | Portal / X11              |

- **Neue Plattform-Crate `fmshot-media`:** Bildschirm als Video, Webcam,
  Mikrofon, Kodieren und Dekodieren hinter Traits, Implementierungen in
  `src/<plattform>.rs`. Der Bildschirm-Stream liegt hier und nicht in
  `fmshot-capture`, weil die Bilder ohne Kopie direkt zum Encoder gehen
  (macOS: IOSurface → AVAssetWriter). `fmshot-capture` bleibt bei
  Einzelbildern; die Mausspur kommt mit Etappe 5 dorthin.
- **Mikrofon über das System statt `cpal`:** Unter macOS liefert
  AVFoundation Tonblöcke mit derselben Systemuhr wie ScreenCaptureKit. So
  passen Bild und Ton ohne Abgleich zusammen. `cpal` liefert eigene
  Zeitstempel, die erst umgerechnet werden müssten.
- **Zeitachse:** Der Stream läuft schon während des Countdowns. Erst der
  Start legt den Nullpunkt fest; das letzte Bild davor wird das erste Bild
  des Videos (ein ruhiger Bildschirm liefert sonst lange kein neues Bild).
  Pausen werden aus allen Spuren gleich herausgerechnet.
- **Neue Crate `fmshot-video`**, plattformunabhängig, hängt intern nur von `core`
  und `render` ab:
  - Zeitachse und Schnitt
  - Webcam-Layout
  - Kopfverfolgung mit Glättung
  - Auto-Zoom aus der Mausspur
  - Export-Ablauf
  - Sie entscheidet, was in jedem Bild wo liegt. Editor-Vorschau und Export
    nutzen dieselbe Rechnung.
  - Sie kennt Dekoder und Encoder nur als Schnittstellen (`FrameSource`,
    `FrameSink`); `fmshot-media` setzt sie mit den Bordmitteln des Systems um
    und hängt dafür von `fmshot-video` ab (ohne die GPU-Funktion). Der
    Export lässt sich so mit erzeugten Bildern testen.

### Webcam
- **Ein/aus** vor und während der Aufnahme; Kamera wählbar.
- **Lage:** oben links, oben rechts, unten links, unten rechts, frei verschoben.
  Grösse in Prozent der Bildbreite.
- **Form:** Kreis, Quadrat, Querformat 16:9, Hochformat; Eckenradius 0–50 %;
  Rahmen und Schatten wahlweise.
- **Ausschnitt:** *Vollbild* (ganzes Kamerabild) oder *Kopf*: ein Ausschnitt um
  das erkannte Gesicht. *Verfolgen* an/aus: Der Ausschnitt folgt dem Kopf weich
  (gedämpfte Feder, kein Zittern); aus = einmal ausgerichtet, dann fest.
- **Ganze Fläche:** Webcam und Bildschirm lassen sich abschnittsweise tauschen
  (Webcam gross, Bildschirm klein oder ganz weg), auch während der Aufnahme.
- **Hintergrund:** unverändert, unscharf oder entfernt.
- **Live-Blase:** Während der Aufnahme schwebt die Webcam als verschiebbares
  Fenster auf dem Bildschirm. Sie ist von der Bildschirmaufnahme ausgenommen
  (eigene Spur), ihre Lage wird das Startlayout.
- **Modelle** (ONNX über `ort`, wie ADR 0010, je unter 1 MB, mitgeliefert):
  - Gesicht: YuNet (MIT)
  - Hintergrund: MediaPipe Selfie Segmentation (Apache-2.0)

  Die Erkennung läuft schon bei der Aufnahme für die Blase. Gespeichert werden
  nur die Kopfpositionen (`faces.json`). Die Hintergrundmaske wird beim
  Export neu berechnet.

### Mauszeiger, Klicks, Auto-Zoom
- Ab Etappe 5 wird der Bildschirm **ohne** Mauszeiger aufgenommen (bis dahin
  mit Zeiger). FMShot zeichnet ihn dann aus `input.json` selbst. Dadurch sind möglich:
  - Zeiger vergrössern, glätten oder ausblenden
  - Klicks als Kreis hervorheben
- **Auto-Zoom:** Aus den Klicks entstehen Zoom-Abschnitte (weich hinein auf
  den Klickbereich, wieder hinaus). In der Zeitleiste lassen sie sich
  verschieben, löschen und von Hand ergänzen.

### Editor und Export
- Das Video öffnet im bestehenden Editor, mit einer Zeitleiste darunter:
  - Trimmen, Abschnitte herausschneiden
  - Webcam-Layout und Zoom je Abschnitt
  - Pfeile, Text und andere Werkzeuge mit Ein- und Ausblendzeit
  - Ein Einzelbild als Screenshot in die Bibliothek
- **Vorschau im Editor:** Die Webview spielt `screen.mp4` und `webcam.mp4` mit
  `<video>` ab. Lage, Maske und Zoom kommen aus `fmshot-video` (WASM), die
  Ebenen zeichnet `fmshot-render` (WASM) darüber.
- **Export:** nativ, ohne Webview:
  1. Dekodieren über das System
  2. Zusammensetzen auf der Grafikkarte mit `wgpu` (MIT/Apache-2.0): Skalieren,
     Zoom, Webcam-Maske, Hintergrund
  3. Vektor-Ebenen und Zeiger zeichnet `fmshot-render` einmal und lädt sie als
     Textur hoch (ADR 0002 sieht den GPU-Weg hinter dem Trait vor)
  4. Kodieren über das System

  Formate:
  - MP4 (H.264 + AAC, Standard; HEVC wählbar)
  - GIF (`gif`-Crate, MIT/Apache-2.0; nicht `imagequant`, das ist GPL)
- Snapshot-Tests: Für feste Zeitpunkte wird ein Bild exportiert und mit einer
  Referenz verglichen, wie bei `fmshot-render`.

### Bedienung
- **Tray-Fenster:** Ein Klick aufs Symbol öffnet ein kleines Fenster mit den
  Tabs *Bild*, *Video* und *Einstellungen* (Webview, damit es überall gleich
  aussieht; angelegt beim ersten Klick, danach nur versteckt). Rechtsklick
  zeigt das klassische Menü. Unter Linux meldet das Tray keine Klicks, dort
  bleibt das Menü.
- **Ziel:** Bereich, Fenster oder ganzer Bildschirm (im Tab *Video* gewählt).
- Tastenkombination **Video** ⌘⇧6 / Ctrl+Shift+6:
  1. Auswahl-Overlay wie bei Bereich und Fenster (Vollbild: Monitor unter der Maus)
  2. Countdown 3 s (abschaltbar) in der Aufnahmeleiste
  3. Nochmals drücken = Stopp
- Mikrofon, Systemton, Countdown und Bilder pro Sekunde stellt der Tab
  *Video* ein (`settings.video`).
- Während der Aufnahme eine kleine Leiste: Stopp, Pause, Webcam ein/aus,
  Webcam gross/klein, Abbrechen. Zeit und Pegel sichtbar.
- Danach gilt die Einstellung „Nach der Aufnahme“ wie bei Bildern.
- Rechte unter macOS: Kamera und Mikrofon (`NSCameraUsageDescription`,
  `NSMicrophoneUsageDescription`, Entitlement `device.audio-input` bzw.
  `device.camera` wegen Hardened Runtime). Beim ersten Mal fragt das System
  selbst; ist die Freigabe verweigert, öffnet FMShot direkt die passende
  Stelle der Systemeinstellungen – nie ein eigener Hinweis.

### Nachtrag Etappe 2: gewählte Standards
- **Abschnitte:** Webcam ein/aus und gross/klein schreiben während der
  Aufnahme Abschnitte (`video.webcam.sections` in `<id>.json`, Zeiten in ms
  als `u32`). Übergang 300 ms (weich): Die hintere Spur steht sofort an
  ihrem Ziel, die vordere gleitet bzw. blendet – so scheint nie Schwarz durch.
- **Webcam aus** stoppt die Kamera (Lämpchen aus). Die Spur entsteht beim
  ersten Einschalten und beginnt am gemeinsamen Nullpunkt; die Kamera läuft
  parallel zum Countdown an (bis ~1 s), der Start wartet nicht auf sie.
- **Kamera:** höchstens 1080p (Preset 1920×1080, sonst 1280×720), HEVC
  mit ≈ 0,1 bit/Pixel (2–16 Mbit/s).
- **Layout-Standard:** Kreis, 20 % der Bildbreite, unten rechts, Abstand
  3 % der kürzeren Bildseite, weisser Rahmen 2,5 %, Schatten, gespiegelt.
  Eckenradius sonst 12 % der kürzeren Seite. Grösse 5–60 % der Breite.
- **Webcam gross:** Webcam füllt das Bild (mittig beschnitten), der
  Bildschirm liegt mit mindestens 28 % Breite in der Ecke der Webcam.
- **Live-Blase:** 80–640 Punkte breit, Scrollrad ändert die Grösse. Ihre
  Lage beim Stopp wird das Startlayout; liegt sie höchstens 8 % der kürzeren
  Seite neben einer Ecke oder ganz ausserhalb des Bereichs, rastet sie in
  der (nächsten) Ecke ein, sonst bleibt sie frei.
- **Export:** H.264 High mit ≈ 1,6× der Datenrate des Bildschirmvideos,
  gleiche Bildrate wie die Aufnahme, Ton gemischt (AAC 160 kbit/s). Schlägt
  das Zusammensetzen fehl (keine GPU), entsteht das Video wie in Etappe 1
  ohne Webcam.

### Nachtrag Etappe 2: Erweiterungspunkte für Etappe 3–6
- `fmshot_video::layout::scene_at` liefert pro Bild eine `Scene`: Spuren
  (`Source`) mit `Placement` (Ziel, Ausschnitt `crop`, Radius, Rahmen,
  Schatten, Deckkraft). **Kopfausschnitt** (3) setzt nur `crop` der Webcam
  aus `faces.json`; **Zoom** (5) setzt `crop` des Bildschirms.
- **Hintergrundmaske** (3): zusätzliche Textur je Spur im Compositor
  (`gpu.rs`, Bindung 3) und Mischung im Shader `compose.wgsl`.
- **Zeiger und Ebenen** (5, 6): weitere `Source`-Arten, deren Bild
  `fmshot-render` zeichnet und `Compositor::upload` hochlädt; sie kommen
  oben auf die Szene. Ebenen mit Zeitbereich filtert `scene_at` nach `t`.
- Snapshot-Tests (`crates/fmshot-video/tests/snapshots.rs`) laufen mit
  erzeugten Spuren; neue Fälle dort ergänzen.

### Nachtrag Etappe 3: Kopf und Hintergrund
- Datenmodell: `framing` Ganzes Bild/Kopf, `follow`, `background`
  Original/unscharf/entfernt im Webcam-Layout; `FaceTrack` in
  `<id>_faces.json`. Kopfausschnitt mit kritisch gedämpfter Feder und
  Totzone (`fmshot_video::head`), Personen-Maske im Shader (Weichzeichnen
  mit 48 Abtastpunkten, Entfernen bzw. dunkle Fläche bei grosser Webcam).
- **Erkennung unter macOS mit Apple Vision statt ONNX** (Gesicht:
  `VNDetectFaceRectanglesRequest`, Person: `VNGeneratePersonSegmentationRequest`,
  Qualität *balanced* – von Apple für Video gedacht): kein Modell-Download,
  läuft auf der Neural Engine. Für Windows/Linux kommt ein ONNX-Modell hinter
  denselben Schnittstellen (`Segmenter`, Gesichter als `FaceTrack`).
- **Während der Aufnahme** erkennt `fmshot-media` das grösste Gesicht etwa
  10× pro Sekunde, immer wenn die Webcam läuft – so lässt sich der
  Kopfausschnitt auch nachträglich einschalten. Zeit ohne Pausen, Rechteck
  oben links, nicht gespiegelt; beim Stopp nach `<id>_faces.json`
  (`RecordFiles::faces`).
- **Eigene Warteschlange:** Erkennung und Blasenbild laufen nicht im
  Callback der Kamera. Ist die Warteschlange noch beschäftigt, fällt das
  Bild nur für Erkennung und Blase aus; die Webcam-Spur bekommt jedes Bild.
- **Live-Blase:** Bei Ganzes Bild + Original bleibt es beim
  `AVCaptureVideoPreviewLayer` (ohne Verzögerung). Sonst füllt
  `fmshot-media` eine eigene Ebene mit CoreImage: Zuschnitt aus
  `fmshot_video::LiveHead` (dieselbe Feder wie im Export; ohne Folgen der
  Median der ersten 20 Erkennungen), Maske mit `CIBlendWithMask`
  (Gauss mit 2 % der Breite ≈ Scheibe des Shaders), höchstens 720 px.
  Bei *Entfernen* ist die Blase durchsichtig, ohne Rahmen und Schatten –
  wie im Export.
- **Export:** `fmshot_media::segmenter()` liefert die Maske je Webcam-Bild
  (BGRA → `CVPixelBuffer` → Maske R8); die App setzt ihn nur, wenn der
  Hintergrund nicht *Original* ist.
- **Vorschau im Editor:** Den Kopfausschnitt rechnet die WASM-Vorschau mit
  derselben `HeadPath` aus `<id>_faces.json` wie der Export. Hintergrund
  unscharf oder entfernt zeigt vorerst nur das fertige Video.
- Die Schalter (Ausschnitt, Kopf folgen, Hintergrund) stehen im Tray-Tab
  *Video* und gelten für die nächste Aufnahme; im Editor sind sie noch nicht
  änderbar.

### Nachtrag Etappe 4–6: Video-Editor, Zeiger, Ebenen
- **Ein Editor für Bild und Video:** Ein Video öffnet in der normalen
  Leinwand. An der Stelle der Aufnahme (unterste Bild-Ebene) liegt die
  Vorschau aus `<video>`-Elementen; Pfeile, Text usw. zeichnet wie immer
  `fmshot-render`. Werkzeuge, die die Bildpunkte darunter brauchen
  (Weichzeichnen, Lupe, Spotlight) oder die Zeichenfläche ändern
  (Zuschneiden, Hintergrund), fehlen im Video.
- **Zeitachse:** Alles im Dokument steht in Zeit der Aufnahme; Schnitte
  (`video.cuts`) fallen im fertigen Video weg (`fmshot_video::timeline`).
  Editor (WASM `VideoPreview`) und Export rechnen dieselbe Umrechnung.
  Bearbeiten (`fmshot_video::edit`) gibt immer ein neues Dokument zurück –
  Rückgängig funktioniert wie bei Bildern.
- **Fertiges Video:** Nach jeder Änderung setzt FMShot `<id>_video.mp4`
  1,5 s später im Hintergrund neu zusammen (ein neuer Auftrag bricht den
  alten ab) und ersetzt die Datei erst am Ende. Kopieren, Sichern und
  Einzelbild warten darauf.
- **Ton:** Lautstärke je Spur 0–1 (AVAudioMix kann nicht verstärken), die
  bleibenden Stücke werden hintereinandergehängt.
- **GIF:** höchstens 960 px, 12 Bilder/s, Farben je Bild (NeuQuant).
- **Mauszeiger (Etappe 5):** Aufgenommen wird ohne Zeiger; die Mausspur
  (`<id>_input.json`, ~60 Proben/s, Lage relativ zum Bereich, Taste
  gedrückt) liest nur den Zustand (`CGEventSourceButtonState`) und braucht
  keine Freigabe. Der Zeiger ist ein eigener Pfeil (Form wie macOS, keine
  Wechsel zu I-Balken usw.), Grösse 1–3×, Klicks als Kreis (600 ms).
  Beim Fenster-Ziel gilt die Fensterlage beim Start.
- **Zoom:** Abschnitte mit Mitte und Stärke (1–4×), 500 ms weich hinein und
  hinaus; Zeiger und Klicks folgen dem Zoom. *Auto-Zoom* fasst Klicks mit
  weniger als 3 s Abstand zusammen (0,7 s Vorlauf, 1,8 s Nachlauf, 1,8×).
- **Ebenen mit Zeitbereich (Etappe 6):** `video.layer_times`; neue Ebenen
  sind ab der aktuellen Stelle 4 s zu sehen, „Ab hier“/„Bis hier“/„Immer“
  ändern das. Im Export zeichnet `fmshot_video::overlay` die sichtbaren
  Ebenen nur neu, wenn sich die Auswahl ändert, und legt sie oben auf.
- **Compositor:** Ein gemeinsamer Uniform-Puffer mit dynamischem Versatz
  (bis 64 Ebenen pro Bild); Texturen bringen ihre Deckkraft mit (Zeiger,
  Kreise, Ebenen).

### Nachtrag: Nur Webcam
- **Tray-Tabs:** *Bild · Bildschirm · Webcam · Einstellungen*. *Bildschirm*
  ist der frühere Tab *Video*; *Webcam* nimmt nur die Kamera auf.
- **Start:** Knopf im Tab, Tray-Menü oder ⌘⇧7 / Ctrl+Shift+7 (nochmals =
  Stopp). Keine Bildschirm-Freigabe, kein Systemton, keine Mausspur.
- **Format:** Quer 16:9, Hoch 9:16 oder Quadrat 1:1 (`VideoDoc.camera_only`).
  Die kürzere Seite wie die Kamera, höchstens 1080 Pixel (Hochformat
  1080×1920). Die Webcam füllt das Bild; Kopfausschnitt, Hintergrund und
  Spiegeln wie bei der Blase, eigene Einstellungen (`settings.video.camera`).
- **Aufnahme:** `RecordOptions::target = None`, `RecordFiles::screen = None`.
  Die Webcam-Spur ist dann die Hauptspur; die Aufnahme beginnt erst, wenn die
  Kamera läuft. Die Blase steht mitten auf dem Monitor unter der Maus, im
  Seitenverhältnis des Formats.
- **Export:** An der Stelle des Bildschirms steht `BlankSource` (gibt nur die
  Grösse vor); `scene_full` legt nur die Webcam ins Bild. Im Editor gibt die
  Webcam den Takt vor; Zoom und Webcam-Lage fehlen dort.

### Nachtrag: Einstellungen für das fertige Video
- Einstellungen → *Video*: **Auflösung** (wie aufgenommen, 2160p, 1440p,
  1080p, 720p = kürzere Seite höchstens so lang, nie vergrössert),
  **Codec** (H.264 Standard, HEVC wählbar), **Qualität** (klein ½×,
  Standard, hoch 2× der Datenrate; HEVC 0,6× von H.264, 1–100 Mbit/s) und
  Bilder pro Sekunde (`settings.video.encoding`, `VideoEncoding` in `core`).
- Aufgenommen wird weiter in voller Qualität. Die Einstellung gilt beim
  Zusammensetzen (`fmshot_video::export_size`, `ExportExtras::size`,
  `fmshot_media::create_mp4`) – direkt nach der Aufnahme und nach jeder
  Änderung im Editor. Der Ausweg ohne GPU (`export_mp4`) bleibt beim
  Standard.
- Das Dokument bezieht sich immer auf die volle Grösse (`save_video` bekommt
  sie getrennt vom Standbild); so passen Ebenen bei jeder Auflösung.

### Nachtrag: Live zeichnen während der Aufnahme
- **Bedienung:** Knopf *Zeichnen* in der Aufnahmeleiste oder ⌘⇧8 /
  Ctrl+Shift+8. Links erscheint eine Werkzeugleiste: Pfeil, Linie, Rahmen,
  Kreis, Freihand, Nummer, Textmarker (Tasten wie im Editor: A, L, R, O, P,
  N, H), vier Farben und *Fertig*. Shift rastet Linien in 45° ein und macht
  Rahmen quadratisch. Kein Text (zu langsam während einer Aufnahme).
- **Verschwinden:** Was zu sehen ist, bildet eine Gruppe. Sie verschwindet
  3 s nach der letzten Zeichnung (Uhrzeit, läuft auch in der Pause) oder
  sofort mit Esc; ein zweites Esc beendet das Zeichnen. Danach beginnen die
  Nummern wieder bei 1.
- **Nichts eingebrannt:** Die Zeichenfläche ist ein Panel von FMShot und
  damit nicht im Bildschirmvideo. Jede Zeichnung wird eine Ebene mit
  Zeitbereich (`video.layer_times`) – im Editor verschieb- und löschbar.
  Das erste Zusammensetzen nach der Aufnahme zeichnet sie schon mit.
- **Logik** in `fmshot_video::sketch::LiveSketch` (plattformunabhängig,
  getestet), **Fläche** in `fmshot-overlay` (`DrawSurface`; macOS: Panel
  unter Leiste und Blase, Werkzeuge als SF Symbols). Auf dem Bildschirm
  zeichnet `fmshot-render` in Punkt-Auflösung; das Video in voller Grösse.
- Textmarker mit 45 % Deckkraft: Auf durchsichtigem Grund wirkt
  „Multiplizieren“ nicht.
- **Pause** hat jetzt auch ein Kürzel: ⌘⇧9 / Ctrl+Shift+9.
- **Lupe** folgt: Sie braucht ein Live-Bild des Bildschirms in der Fläche
  und im Export eine zusätzliche Bildschirm-Lage im Compositor.

### Nachtrag: Zeitleiste mit Zeilen je Werkzeug
- Unter Video, Webcam und *Vergrössern* (früher „Zoom“, mit Erklärung beim
  Darüberfahren) steht je Werkzeug eine Zeile: Pfeile, Linien, Rahmen,
  Kreise, Freihand, Nummern, Textmarker, Text, Sprechblasen, Bilder,
  Weitere. Jede Ebene ist ein Block in ihrer Farbe (Nummer bzw. Textanfang
  als Beschriftung); überlappende Blöcke kommen in weitere Zeilen derselben
  Gruppe (`layerLanes` in `videoLayers.ts`, reine Darstellung).
- **Block:** Klick wählt die Ebene (und springt hin, wenn sie gerade nicht
  zu sehen ist), Ziehen verschiebt, die Ränder ändern die Länge
  (mindestens 100 ms). Übernommen wird beim Loslassen – ein Schritt fürs
  Rückgängigmachen (`layerTime` wie bisher). Ebenen ohne Zeitbereich
  liegen blass über die ganze Länge.
- Unter der Zeitleiste: Ab hier / Bis hier / Wie Auswahl / Immer und
  **Löschen**. Zeitbereiche gelöschter Ebenen werden im selben Schritt
  entfernt, sonst erbte eine neue Ebene mit derselben Nummer ihren Bereich.
- Neue Textmarker im Video haben 45 % Deckkraft (wie live gezeichnete).

### Budgets (Apple M1)
| Messpunkt                                  | Ziel              |
|--------------------------------------------|-------------------|
| Aufnahme-Knopf → erstes Bild aufgezeichnet | < 300 ms          |
| Prozessorlast 1080p30 + Webcam + Ton       | < 20 % (ein Kern = 100 %) |
| Export 1080p30 mit Webcam und Zoom         | ≥ 2× Echtzeit     |
| Versatz Ton ↔ Bild                         | < 40 ms           |

## Folgen
- Videos können viel Speicher belegen (1080p30 HEVC rund 0,5–1 GB pro Stunde je
  Videospur). Die Aufbewahrungsregel gilt auch für Videos.
- Der Export braucht Zeit. Dafür bleibt alles bis zuletzt änderbar.
- Vorschau (Webview) und Export (wgpu) sind zwei Wege zum Bild. Damit sie
  nicht auseinanderlaufen, rechnet `fmshot-video` die Lage für beide, und die
  Snapshot-Tests decken die Unterschiede auf.
- Unter Linux spielt die Webview Videos über das GStreamer des Systems ab. Das
  ist keine Abhängigkeit von FMShot.
- `wgpu` vergrössert die App um einige MB.

## Etappen (je ein Meilenstein-Issue)
1. **Bildschirm + Ton:** Video-Modus, Aufnahmeleiste mit Pause und Countdown,
   `screen.mp4`, `system.m4a`, `mic.m4a`, Eintrag in der Bibliothek,
   Export als MP4 ohne Bearbeitung.
2. **Webcam:** Spur, Live-Blase, Lage, Form, Rahmen, Export zusammengesetzt
   (wgpu).
3. **Kopf und Hintergrund:** Gesichtserkennung, Kopfausschnitt, Verfolgen,
   Hintergrund unscharf oder entfernt.
4. **Video-Editor:** Zeitleiste, Trimmen, Schneiden, Layout je Abschnitt,
   Webcam gross/klein tauschen, GIF, Einzelbild.
5. **Mauszeiger:** eigener Zeiger, Klicks hervorheben, Auto-Zoom.
6. **Ebenen im Video:** Pfeile, Text usw. mit Zeitbereich.
