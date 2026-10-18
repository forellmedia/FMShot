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
