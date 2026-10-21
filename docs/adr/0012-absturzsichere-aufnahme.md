# 0012 – Absturzsichere Video-Aufnahme

**Status:** angenommen (ergänzt ADR 0011)

## Kontext
Eine lange Aufnahme darf nicht verloren gehen, wenn Grabmark abstürzt, hängt oder
beendet wird. Gewünscht war ein Format wie MPEG-TS (`.ts`), das sich bis zum
Abbruch lesen lässt. Ausserdem soll man während der Aufnahme sehen, dass sie
läuft, und merken, wenn sie abbricht.

Bisher: Bildschirm und Webcam wurden schon als fragmentiertes MP4 geschrieben,
der Ton (`.m4a`) nicht – nach einem Absturz war er unlesbar („Cannot Open –
This media may be damaged“, nachgeprüft mit einem hart abgebrochenen Schreiber).
Mausspur, Webcam-Abschnitte und Live-Zeichnungen lagen nur im Speicher, und
niemand suchte nach liegengebliebenen Aufnahmen.

## Entscheidung

### Fragmentiertes MP4 statt `.ts`
AVFoundation schreibt kein MPEG-TS; ein eigener Muxer wäre viel Arbeit, FFmpeg
ist LGPL (ADR 0008). Fragmentiertes MP4 leistet dasselbe: Alle Spuren
(Bildschirm, Webcam, Systemton, Mikrofon) schreiben alle **2 s** ein Fragment.
Nach einem harten Abbruch sind sie bis zum letzten Fragment lesbar (geprüft:
9 s geschrieben, `abort()` → Ton 7,96 s, Bild 8,2 s lesbar).

Scheitert ein Schreiber unterwegs (Platte voll, Kodierer), bleibt seine Datei
stehen statt gelöscht zu werden, und die Aufnahme endet sichtbar.

### Zwischenstände neben den Spuren
```
<id>_recording.json   Zwischenstand: fps, Skalierung, nur Webcam, Webcam-Layout,
                      Webcam-Abschnitte, fertige Live-Zeichnungen
                      (`RecordingCheckpoint`, bei jeder Änderung atomar neu)
<id>_input.jsonl      Mausspur, jede Sekunde neue Zeilen angehängt
```
Nach dem erfolgreichen Sichern werden beide gelöscht. Der Gesichtsverlauf
(`_faces.json`) wird nicht laufend gesichert – nach einem Absturz zeigt der
Kopfausschnitt die ganze Webcam.

### Wiederherstellen beim Start
Eine Aufnahme, die nie fertig wurde, erkennt die Bibliothek am leeren
Platzhalter `<id>.json` ohne Index-Eintrag (`Library::unfinished_videos`).
Mit Bildschirm- oder Webcam-Spur fragt Grabmark beim Start: „Wiederherstellen“,
„Verwerfen“ oder „Später“ (Esc/Schliessen = später, damit nichts aus Versehen
verloren geht). Wiederherstellen baut das Dokument aus Zwischenstand und
Spurlänge (`grabmark_video::recovery`) und nimmt denselben Weg wie nach einer
normalen Aufnahme – nur ohne Zwischenablage und Vorschau.

### Sichtbarer Zustand
- Roter Rahmen bei jedem Ziel: Bereich, Fenster (folgt dem Fenster) und
  Vollbild (am Monitorrand nach innen). Grabmark ist im Video nie zu sehen.
- Grosse Zeit mit gleich breiten Ziffern in der Leiste, roter Punkt (grau in
  der Pause), Pegel klein daneben.
- Bricht die Aufnahme ab (Stream gestoppt, Schreibfehler, unter 500 MB frei):
  Rahmen und Text orange („Unterbrochen – gesichert bis 3:12“), das Video wird
  gesichert, danach ein Hinweis. Unter 2 GB frei warnt die Leiste.
- Beenden über Menü oder Tray sichert eine laufende Aufnahme erst.
- Hängt Grabmark, bleibt die Zeit sichtbar stehen; stürzt es ab, verschwinden
  Leiste und Rahmen – beides ist selbst ein Zeichen.

## Folgen
- Höchstens die letzten ~2 s gehen bei einem Absturz verloren.
- Fragmente kosten etwas Platz und beim Abschliessen kurz Zeit.
- Windows/Linux: Video-Aufnahme gibt es dort noch nicht; die Schnittstellen
  (`free_space`, `duration`, `window_bounds`, Leiste) sind vorhanden und tun
  dort vorerst nichts.
