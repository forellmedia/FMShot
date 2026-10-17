# 0009 – Erst Apple Silicon, dann Windows und Linux

**Status:** angenommen (ersetzt in ADR 0007 die macOS-Architektur „Universal“)

## Kontext
Nach dem ersten Test der App gibt es noch viele Baustellen im Editor und bei der
Aufnahme. Jede Runde über GitHub (Push, CI, Release, Update) ist langsam. Intel-
Macs bringen zusätzliche Build-Zeit und doppelte Binärgrösse, aber keinen Nutzen
für die aktuelle Phase.

## Entscheidung
- **Phase „Apple Silicon“**: FMShot wird zuerst nur auf Macs mit Apple Silicon
  (arm64) fertig gemacht. Die Phase endet, wenn der Nutzer sie für beendet
  erklärt.
- **Intel-Macs fallen weg** – kein x86_64-Build, keine Unterstützung. Mindestens
  weiterhin macOS 14.
- **Arbeitsablauf**: ändern → lokal bauen und installieren
  (`scripts/build-local.sh`) → der Nutzer testet → erst nach seiner Freigabe
  committen und pushen.
- **Windows und Linux** bleiben im Code und in der CI:
  - Alles muss weiter für Windows und Linux kompilieren, die Tests laufen
    weiter. Das hält Regel 2 aus CLAUDE.md ehrlich (Plattformcode hinter Traits).
  - Neue Plattformfunktionen werden für macOS gebaut. Für Windows/Linux wird die
    Implementierung gleich mitgeschrieben, wenn das wenig Aufwand ist – sonst
    liefert sie einen klaren Fehler „auf dieser Plattform noch nicht
    unterstützt“ und wird nach der Phase nachgezogen.
  - Getestet wird in dieser Phase nur auf dem Mac.
- Die Release-Pipeline baut für macOS nur noch `aarch64-apple-darwin`.

## Folgen
- Kleinere App, schnellerer Build, schnellere Runden.
- Wer FMShot auf einem Intel-Mac installiert hat, bekommt keine Updates mehr.
- Nach der Phase braucht es eine eigene Runde „Windows/Linux nachziehen“ mit
  Tests in den UTM-VMs.
