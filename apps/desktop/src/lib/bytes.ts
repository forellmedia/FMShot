// Dateigrössen für Menschen: 1 KB = 1000 Bytes, wie im Finder.

const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

/** „7,0 GB“, „380 MB“, „12 KB“ – eine Nachkommastelle erst ab GB. */
export function formatBytes(bytes: number | null, locale = "de"): string {
  let value = Math.max(0, bytes ?? 0);
  let unit = 0;
  while (value >= 1000 && unit < UNITS.length - 1) {
    value /= 1000;
    unit++;
  }
  const digits = unit >= 3 && value < 100 ? 1 : 0;
  const n = new Intl.NumberFormat(locale, { minimumFractionDigits: digits, maximumFractionDigits: digits }).format(value);
  return `${n} ${UNITS[unit]}`;
}

/** Anteil an der Grenze (0–∞); ohne Grenze 0. */
export function share(bytes: number | null, limit: number | null): number {
  return limit ? (bytes ?? 0) / limit : 0;
}
