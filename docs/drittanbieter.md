# Drittanbieter-Komponenten

FMShot nutzt Open-Source-Bibliotheken unter freizügigen Lizenzen
(MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0, Unicode). `cargo deny check licenses`
prüft das bei jedem Build.

## Schrift
- **Inter** © The Inter Project Authors – SIL Open Font License 1.1.
  Lizenztext: [crates/fmshot-render/fonts/Inter-LICENSE.txt](../crates/fmshot-render/fonts/Inter-LICENSE.txt).
  Die Schrift ist im Renderer eingebettet.
- **Twemoji** (Emoji-Grafiken) © Twitter, Inc. und weitere Mitwirkende –
  [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/); als COLR-Schrift
  „Twemoji Mozilla“ © Mozilla Foundation – Apache-2.0
  ([mozilla/twemoji-colr](https://github.com/mozilla/twemoji-colr), v0.7.0, unverändert).
  Lizenztext: [crates/fmshot-render/fonts/Twemoji-LICENSE.md](../crates/fmshot-render/fonts/Twemoji-LICENSE.md).
  Die Schrift ist im Renderer eingebettet.

## JPEG-Export
- Dieses Programm basiert teilweise auf der Arbeit der Independent JPEG Group
  (über `jpeg-encoder`).
