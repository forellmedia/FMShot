// Zwischenspeicher der Leinwand beim Ziehen (ADR 0002): drei übereinander
// liegende Flächen – Ebenen unter der aktiven, die aktive selbst und die
// darüber. Zu Beginn einer Zieh-Aktion werden „unten“ und „oben“ einmal
// gezeichnet, danach pro Bild nur noch die aktive Ebene.

export type Surface = "base" | "active" | "above";

/** Zeichnet auf die Flächen der Leinwand. */
export interface Painter {
  /** Fläche leeren und die Ebenen `from..to` zeichnen (`to` < 0 = bis zum Ende). */
  draw(surface: Surface, from: number, to: number, background: boolean): void;
  clear(surface: Surface): void;
}

export class LayerCache {
  /** Index der aktiven Ebene; `fast` = Zwischenspeicher wird benutzt. */
  private session: { index: number; fast: boolean } | null = null;
  /** Flächen „unten“/„oben“ passen nicht mehr (geleert oder andere Ansicht). */
  private stale = false;

  constructor(private painter: Painter) {}

  /** Zieh-Aktion an Ebene `index`. `fast` = über ihr liegt kein Effekt. */
  begin(index: number, fast: boolean) {
    this.session = { index, fast };
    if (fast) this.fill();
  }

  /** Zieh-Aktion über mehrere Ebenen: jedes Mal alles neu zeichnen. */
  beginSlow() {
    this.session = { index: -1, fast: false };
  }

  end() {
    this.session = null;
  }

  /** Flächen wurden geleert (neue Grösse leert jede Canvas) oder die
   *  Ansicht hat sich verschoben: beim nächsten Bild neu füllen. */
  invalidate() {
    this.stale = true;
  }

  /** Ein Bild zeichnen: beim Ziehen nur die aktive Ebene, sonst alles. */
  render() {
    const s = this.session;
    if (s?.fast) {
      if (this.stale) this.fill();
      else this.painter.draw("active", s.index, s.index + 1, false);
      return;
    }
    this.stale = false;
    this.painter.draw("base", 0, -1, true);
    this.painter.clear("active");
    this.painter.clear("above");
  }

  private fill() {
    const { index } = this.session!;
    this.stale = false;
    this.painter.draw("base", 0, index, true);
    this.painter.draw("above", index + 1, -1, false);
    this.painter.draw("active", index, index + 1, false);
  }
}
