// Fehler aus den Fenstern landen im selben Log wie die des Backends – sonst
// sieht man sie nur im Web-Inspektor, und der ist in der App aus.

import { logToBackend } from "$lib/api";

function describe(value: unknown): string {
  if (value instanceof Error) return value.stack ?? `${value.name}: ${value.message}`;
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

let installed = false;

/** Unbehandelte Fehler, abgelehnte Promises und `console.error/warn` weiterleiten. */
export function installErrorLogging() {
  if (installed || typeof window === "undefined") return;
  installed = true;
  const where = () => location.pathname || "/";

  window.addEventListener("error", (e) => {
    const at = e.filename ? ` (${e.filename}:${e.lineno}:${e.colno})` : "";
    logToBackend("error", `${where()}: ${describe(e.error ?? e.message)}${at}`);
  });
  window.addEventListener("unhandledrejection", (e) => {
    logToBackend("error", `${where()}: unbehandelt: ${describe(e.reason)}`);
  });

  for (const level of ["error", "warn"] as const) {
    const original = console[level].bind(console);
    console[level] = (...args: unknown[]) => {
      original(...args);
      logToBackend(level, `${where()}: ${args.map(describe).join(" ")}`);
    };
  }
}
