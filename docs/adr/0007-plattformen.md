# 0007 – Plattformen und Mindestversionen

**Status:** angenommen

| Plattform | Mindestens                | Architekturen              |
|-----------|---------------------------|----------------------------|
| macOS     | 14 Sonoma                 | Universal (arm64 + x86_64) |
| Windows   | 10 22H2                   | x64, ARM64                 |
| Linux     | Ubuntu 22.04, Fedora 39   | x86_64, aarch64            |

- macOS 14 wegen `SCScreenshotManager` (ScreenCaptureKit); `CGWindowListCreateImage`
  ist veraltet.
- Linux offiziell: GNOME/KDE unter Wayland (xdg-desktop-portal, GlobalShortcuts-
  Portal) und X11. Sonst Kommandozeile für eigene Tastenkürzel.
- ARM64-Builds für Windows/Linux, damit Tests in UTM-VMs auf Apple Silicon
  nativ laufen.
