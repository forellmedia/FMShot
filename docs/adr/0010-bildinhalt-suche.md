# 0010 – Suche nach Bildinhalt mit SigLIP 2

**Status:** angenommen (ändert in docs/produkt.md das Budget „Download-Grösse“)

## Kontext
Die Galerie findet Aufnahmen bisher nur über erkannten Text (ADR 0005). Gesucht
wird aber auch nach dem, was zu sehen ist: „Mann“, „Baum“, „Wald“,
„Diagramm“. Das muss auf macOS, Windows und Linux gleich funktionieren und auf
einem einfachen M1 oder einem älteren Windows-PC flüssig laufen.

Apple Vision (`VNClassifyImageRequest`) gibt es nur auf macOS, unter Windows
und Linux fehlt etwas Vergleichbares. Darum braucht es ein eigenes Modell, das
überall läuft.

## Entscheidung
- **Modell:** Google **SigLIP 2 Base** (`google/siglip2-base-patch16-224`,
  Apache-2.0, mehrsprachig). Quelle sind die fp32-ONNX-Dateien von
  `onnx-community/siglip2-base-patch16-224-ONNX` auf einer festen Revision.
  `scripts/model/convert.py` verkleinert sie, mit festen Paketversionen und
  reproduzierbar:
  - Bild: MatMul-Gewichte 8 Bit (nur die Gewichte), 108 MB
  - Text: Wortschatz-Tabelle 8 Bit, MatMul-Gewichte 4 Bit, 250 MB
  - dazu der Tokenizer `tokenizer.json`, 34 MB

  Zusammen sind das rund 390 MB. Bild und Suchtext landen beide als Vektor
  (768 Zahlen) im selben Raum. Die Ähnlichkeit ist das Skalarprodukt der
  normierten Vektoren.
- **Warum nicht die fertigen int8-Dateien des Repos:** Bei denen sind auch die
  Zwischenwerte quantisiert. Das verschlechtert die Treffer deutlich: Im Test
  mit 8 Fotos und 18 Suchwörtern stimmte nur bei 72 % der Suchwörter das beste
  Bild mit dem vollen Modell überein. Die oben beschriebene Fassung kommt auf
  100 %, bei einer Korrelation der Ähnlichkeiten von 0,996. Die Dateien
  `vision_model_int8.onnx` laufen ausserdem auf dem Prozessor gar nicht
  (`ConvInteger` fehlt).
- **Laufzeit:** ONNX Runtime über die Crate `ort` (MIT/Apache-2.0), vorerst auf
  dem Prozessor. Eine Beschleunigung über CoreML (macOS) oder DirectML
  (Windows) kommt nur, wenn die Messungen sie verlangen.
- **Mitgeliefert, kein Netzwerk:** Das Modell gehört zum App-Paket (Regel 8).
  `scripts/fetch-model.sh` lädt beim Bauen die Originale von Hugging Face,
  wandelt sie um und prüft Quellen und Ergebnis per SHA-256. Ins Git kommt das Modell nicht (`/models/` steht in
  .gitignore). Ins App-Paket kommt es über
  `src-tauri/tauri.models.conf.json`, das `build-local.sh` und die
  Release-Pipeline zusätzlich angeben. Normale `cargo`-Builds und die CI
  brauchen das Modell so nicht. Fehlt es, bleibt die Suche nach Bildinhalt
  aus.
- **Neue Crate `fmshot-vision`:**
  - plattformunabhängig, hängt intern nur von `core` ab
  - Aufgaben: Bild → Vektor, Text → Vektor, Stichwörter aus einer festen
    Begriffsliste
  - Die Bibliothek speichert die Vektoren als BLOB. Das Modell selbst kennt
    sie nicht, nur dessen Kennung, damit ein Modellwechsel ein Neuberechnen
    auslöst.
- **Stichwörter und Schnellfilter:**
  - Jedes Bild wird mit einer festen Liste von rund 300 Begriffen verglichen,
    jeder Begriff mit Wort auf Deutsch und Englisch.
  - Was über einer Schwelle liegt, wird zum Stichwort und kommt in den
    Suchindex.
  - Schnellfilter wie „Personen“ oder „Natur“ sind Gruppen solcher Begriffe.
- **Grundwert gegen Fehltreffer:** Bildschirmfotos mit viel Text passen
  sonst zu fast jedem Suchwort. Darum zählt bei der Suche, wie weit ein Bild
  über seinem eigenen Grundwert liegt, also über dem Mittel aus Füllwörtern
  wie „das“, „und“ oder „the“. Treffer brauchen:
  - mindestens Logit −8,5
  - mindestens 2,5 über dem Grundwert
  - höchstens 4 hinter dem besten Treffer

  Stichwörter (bis zu drei je Bild) brauchen mindestens Logit −6,5 und
  höchstens 3 Abstand zum besten Begriff. Die Begriffe werden mit
  englischen Beschreibungen erkannt („a screenshot of source code“) und
  übersetzt angezeigt. Ein Suchwort, das einem Stichwort entspricht („Boot“),
  findet auch alle Aufnahmen mit diesem Stichwort.
- **Suche:** Ein Suchtext durchsucht wie bisher den erkannten Text. Zusätzlich
  wird er mit den Bildvektoren verglichen. Die Treffer beider Wege werden
  zusammengeführt. Die Galerie zeigt, warum ein Bild gefunden wurde (Text,
  Stichwort oder Bildinhalt).
- **Hintergrund und Speicher:**
  - Die Erkennung läuft nach dem Speichern einer Aufnahme mit niedriger
    Priorität und nie im Aufnahmepfad.
  - Ältere Aufnahmen werden nach und nach nachgeholt.
  - Die Modelle werden erst bei Bedarf geladen und nach einigen Minuten ohne
    Nutzung wieder freigegeben. So bleibt das Leerlauf-Budget (< 120 MB RAM)
    erhalten.
  - In den Einstellungen lässt sich die Erkennung abschalten. Ab Werk ist sie
    an.
- **Budgets:**
  - Bild → Vektor: < 150 ms auf einem M1 (gemessen auf einem M4: rund 45 ms)
  - Text → Vektor: < 50 ms
  - Download-Grösse: neu < 450 MB statt < 25 MB

## Folgen
- Die App wird rund 390 MB grösser. Jedes Update enthält das Modell mit, bis
  der Updater Teil-Updates kann.
- Die Ergebnisse sind auf allen Plattformen gleich, und es gibt nur eine
  Lösung zu pflegen.
- „Mann“ und „Frau“ trennt das Modell nur so gut wie seine Trainingsdaten.
  Feinere Begriffe wie Gesichter oder Namen gibt es nicht.
- Die Modelldateien sind keine Rust-Crates und fallen nicht unter
  `cargo deny`. Ihre Lizenz (Apache-2.0, auch der Tokenizer aus Gemma laut
  Modellkarte) gehört in die Drittanbieter-Hinweise der App.
- Fürs Bauen braucht es `uv` und einmalig rund 1,5 GB Download für die
  Originale. Die liegen danach im Cache `models/.cache/`.
- Später möglich: das Textmodell auf Deutsch und Englisch kürzen. Der
  Wortschatz mit 256’000 Einträgen macht rund 200 der 250 MB aus.
