// Werkzeuge des Editors mit Tastenkürzel und Symbol (Name aus $lib/icons).

import type { MessageKey } from "$lib/i18n";
import type { IconName } from "$lib/icons";

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
  icon: IconName;
  /** Nicht in der Werkzeugleiste (nur per Taste oder über ein anderes Werkzeug). */
  hidden?: boolean;
};

export const TOOLS: Tool[] = [
  { id: "select", key: "v", label: "tool.select", icon: "select" },
  { id: "arrow", key: "a", label: "tool.arrow", icon: "arrow" },
  { id: "line", key: "l", label: "tool.line", icon: "line" },
  { id: "rectangle", key: "r", label: "tool.rectangle", icon: "rectangle" },
  { id: "ellipse", key: "o", label: "tool.ellipse", icon: "ellipse" },
  { id: "pen", key: "p", label: "tool.pen", icon: "pen" },
  { id: "text", key: "t", label: "tool.text", icon: "text" },
  { id: "callout", key: "b", label: "tool.callout", icon: "callout" },
  { id: "step", key: "n", label: "tool.step", icon: "step" },
  { id: "highlight", key: "h", label: "tool.highlight", icon: "highlight" },
  { id: "spotlight", key: "s", label: "tool.spotlight", icon: "spotlight" },
  { id: "magnifier", key: "m", label: "tool.magnifier", icon: "magnifier" },
  // „Unkenntlich machen“: ein Knopf für Weichzeichnen und Verpixeln, Art oben wählbar.
  { id: "blur", key: "u", label: "tool.obscure", icon: "obscure" },
  { id: "pixelate", key: "x", label: "tool.pixelate", icon: "obscure", hidden: true },
  { id: "crop", key: "c", label: "tool.crop", icon: "crop" },
  { id: "background", key: "g", label: "tool.background", icon: "background" },
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
