<script lang="ts">
  import { t, type MessageKey } from "$lib/i18n";
  import { editor } from "./state.svelte";
  import type { Layer } from "./model";

  type Row = { type: "group"; group: number; members: Layer[] } | { type: "layer"; layer: Layer; inGroup: boolean };

  // Oberste Ebene zuerst – wie in Grafikprogrammen üblich. Gruppen bekommen
  // eine Kopfzeile, ihre Ebenen stehen eingerückt darunter.
  const rows = $derived.by((): Row[] => {
    const layers = [...(editor.doc?.layers ?? [])].reverse();
    const out: Row[] = [];
    const seen = new Set<number>();
    for (const l of layers) {
      if (l.group != null && !seen.has(l.group)) {
        seen.add(l.group);
        out.push({ type: "group", group: l.group, members: layers.filter((x) => x.group === l.group) });
      }
      out.push({ type: "layer", layer: l, inGroup: l.group != null });
    }
    return out;
  });

  const selected = $derived(new Set(editor.selectedIds));

  function name(type: string): string {
    return t(`layer.${type}` as MessageKey);
  }

  function selectLayer(e: MouseEvent, id: number) {
    // In der Liste wird die einzelne Ebene gewählt (auch in einer Gruppe).
    editor.select(id, { single: true, add: e.shiftKey || e.metaKey });
  }

  function selectGroup(e: MouseEvent, members: Layer[]) {
    editor.select(members[0].id, { add: e.shiftKey || e.metaKey });
  }

  /** Sichtbar/Gesperrt für alle Mitglieder einer Gruppe umschalten. */
  function toggleAll(members: Layer[], field: "visible" | "locked") {
    const on = !members.every((m) => m[field]);
    const ids = new Set(members.map((m) => m.id));
    editor.commit((d) => {
      for (const l of d.layers) if (ids.has(l.id)) l[field] = on;
    });
  }
</script>

<aside class="panel" aria-label={t("layers.title")}>
  <h2>{t("layers.title")}</h2>
  <ul>
    {#each rows as row (row.type === "group" ? `g${row.group}` : row.layer.id)}
      {#if row.type === "group"}
        <li class="group-row" class:selected={row.members.every((m) => selected.has(m.id))}>
          <button class="name" onclick={(e) => selectGroup(e, row.members)}>
            ▾ {t("layers.group", { number: String(row.group) })}
          </button>
          <button
            class="icon"
            title={t("layers.visible")}
            class:off={!row.members.every((m) => m.visible)}
            onclick={() => toggleAll(row.members, "visible")}>👁</button
          >
          <button
            class="icon"
            title={t("layers.locked")}
            class:off={!row.members.every((m) => m.locked)}
            onclick={() => toggleAll(row.members, "locked")}>🔒</button
          >
          <button class="icon" title={t("layers.up")} onclick={() => editor.moveGroup(row.group, 1)}>▲</button>
          <button class="icon" title={t("layers.down")} onclick={() => editor.moveGroup(row.group, -1)}>▼</button>
        </li>
      {:else}
        {@const l = row.layer}
        <li class:selected={selected.has(l.id)} class:member={row.inGroup}>
          <button class="name" onclick={(e) => selectLayer(e, l.id)}>
            {l.name ?? name(l.kind.type)}
            {#if l.kind.type === "step"}&nbsp;{l.kind.number}{/if}
          </button>
          <button
            class="icon"
            title={t("layers.visible")}
            class:off={!l.visible}
            onclick={() => editor.updateLayer(l.id, (x) => (x.visible = !x.visible))}>👁</button
          >
          <button
            class="icon"
            title={t("layers.locked")}
            class:off={!l.locked}
            onclick={() => editor.updateLayer(l.id, (x) => (x.locked = !x.locked))}>🔒</button
          >
          <button class="icon" title={t("layers.up")} onclick={() => editor.moveLayer(l.id, 1)}>▲</button>
          <button class="icon" title={t("layers.down")} onclick={() => editor.moveLayer(l.id, -1)}>▼</button>
        </li>
      {/if}
    {/each}
  </ul>
</aside>

<style>
  .panel {
    width: 230px;
    border-left: 1px solid var(--border);
    background: var(--surface);
    overflow-y: auto;
    padding: var(--space-2);
  }
  h2 {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
    margin: var(--space-1) var(--space-2) var(--space-2);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: flex;
    align-items: center;
    border-radius: 6px;
  }
  li.selected {
    background: var(--hover);
  }
  li.member .name {
    padding-left: 22px;
  }
  li.group-row .name {
    font-weight: 600;
  }
  button {
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  .name {
    flex: 1;
    text-align: left;
    padding: 6px 8px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .icon {
    width: 24px;
    font-size: 11px;
    opacity: 0.8;
  }
  .icon.off {
    opacity: 0.25;
  }
</style>
