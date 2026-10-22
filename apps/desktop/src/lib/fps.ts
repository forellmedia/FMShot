// Bildraten der Video-Aufnahme: Film (24), PAL (25/50), Bildschirm (30/60).

export const FPS_CHOICES = [24, 25, 30, 50, 60] as const;

export const DEFAULT_FPS = 30;

/** Nächste Bildrate für den Chip im Tray; unbekannte Werte starten vorne. */
export function nextFps(current: number): number {
  const i = FPS_CHOICES.indexOf(current as (typeof FPS_CHOICES)[number]);
  return FPS_CHOICES[(i + 1) % FPS_CHOICES.length];
}
