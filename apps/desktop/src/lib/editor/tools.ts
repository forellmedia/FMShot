// Werkzeuge des Editors mit Tastenkürzel und Symbol (SVG-Pfade, 24×24).

import type { MessageKey } from "$lib/i18n";

export type ToolId =
  | "select"
  | "arrow"
  | "line"
  | "rectangle"
  | "ellipse"
  | "pen"
  | "text"
  | "callout"
  | "step"
  | "highlight"
  | "spotlight"
  | "magnifier"
  | "blur"
  | "pixelate"
  | "crop"
  | "background";

export type Tool = {
  id: ToolId;
  key: string;
  label: MessageKey;
  icon: string;
  /** Nicht in der Werkzeugleiste (nur per Taste oder über ein anderes Werkzeug). */
  hidden?: boolean;
};

export const TOOLS: Tool[] = [
  { id: "select", key: "v", label: "tool.select", icon: "M5 3l13 8-6 1.5L9 19z" },
  { id: "arrow", key: "a", label: "tool.arrow", icon: "M5 19L19 5M10 5h9v9" },
  { id: "line", key: "l", label: "tool.line", icon: "M5 19L19 5" },
  { id: "rectangle", key: "r", label: "tool.rectangle", icon: "M4 6h16v12H4z" },
  { id: "ellipse", key: "o", label: "tool.ellipse", icon: "M12 5c4.4 0 8 3.1 8 7s-3.6 7-8 7-8-3.1-8-7 3.6-7 8-7z" },
  { id: "pen", key: "p", label: "tool.pen", icon: "M4 18c3-6 5 2 8-4s4-8 8-6" },
  { id: "text", key: "t", label: "tool.text", icon: "M5 5h14M12 5v14M9 19h6" },
  { id: "callout", key: "b", label: "tool.callout", icon: "M4 5h16v10H10l-5 4v-4H4z" },
  { id: "step", key: "n", label: "tool.step", icon: "M12 3a9 9 0 110 18 9 9 0 010-18zM11 8l2-1v10" },
  { id: "highlight", key: "h", label: "tool.highlight", icon: "M4 15h16v4H4zM8 5l8 0 0 10-8 0z" },
  { id: "spotlight", key: "s", label: "tool.spotlight", icon: "M3 3h18v18H3zM8 8h8v8H8z" },
  { id: "magnifier", key: "m", label: "tool.magnifier", icon: "M10 4a6 6 0 110 12 6 6 0 010-12zM15 15l5 5" },
  // „Unkenntlich machen“: ein Knopf für Weichzeichnen und Verpixeln, Art oben wählbar.
  { id: "blur", key: "u", label: "tool.obscure", icon: "M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z" },
  { id: "pixelate", key: "x", label: "tool.pixelate", icon: "", hidden: true },
  { id: "crop", key: "c", label: "tool.crop", icon: "M7 3v14h14M3 7h14v14" },
  { id: "background", key: "g", label: "tool.background", icon: "M3 3h18v18H3zM7 7h10v10H7z" },
];

/**
 * Werkzeuge im Video-Editor: Was Bildpunkte darunter braucht (Weichzeichnen,
 * Lupe, Spotlight) oder die Zeichenfläche ändert (Zuschneiden), fehlt.
 */
export const VIDEO_TOOLS: ToolId[] = [
  "select",
  "arrow",
  "line",
  "rectangle",
  "ellipse",
  "pen",
  "text",
  "callout",
  "step",
  "highlight",
];

/** Ist `tool` gerade aktiv? Der Knopf „Unkenntlich machen“ steht für beide Arten. */
export function isActive(tool: Tool, current: ToolId): boolean {
  return tool.id === current || (tool.id === "blur" && current === "pixelate");
}

export function toolByKey(key: string): Tool | undefined {
  return TOOLS.find((t) => t.key === key.toLowerCase());
}

/** Werkzeuge, die mit Ziehen ein Rechteck aufspannen. */
export const BOX_TOOLS: ToolId[] = [
  "rectangle",
  "ellipse",
  "highlight",
  "spotlight",
  "magnifier",
  "blur",
  "pixelate",
  "callout",
  "crop",
];
