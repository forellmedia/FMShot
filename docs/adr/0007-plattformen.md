# 0007 – Plattformen und Mindestversionen

**Status:** angenommen

| Plattform | Mindestens                | Architekturen              |
|-----------|---------------------------|----------------------------|
| macOS     | 14 Sonoma                 | arm64 (Apple Silicon)¹     |
| Windows   | 10 22H2                   | x64, ARM64                 |
| Linux     | Ubuntu 22.04, Fedora 39   | x86_64, aarch64            |

- macOS 14 wegen `SCScreenshotManager` (ScreenCaptureKit); `CGWindowListCreateImage`
  ist veraltet.
- Linux offiziell: GNOME/KDE unter Wayland (xdg-desktop-portal, GlobalShortcuts-
  Portal) und X11. Sonst Kommandozeile für eigene Tastenkürzel.
¹ Ursprünglich Universal (arm64 + x86_64); Intel fällt weg, siehe ADR 0009.

- ARM64-Builds für Windows/Linux, damit Tests in UTM-VMs auf Apple Silicon
  nativ laufen.
