<script lang="ts">
  // Vorschau eines Videos auf der Leinwand (ADR 0011): Bildschirm und Webcam
  // als <video>, Zeiger und Klicks auf einer Canvas – an der Stelle, die
  // `grabmark-video` rechnet. Was darüber liegt (Pfeile, Text …), zeichnet die
  // Leinwand. Weichzeichnen/Entfernen des Webcam-Hintergrunds ist nur im
  // fertigen Video zu sehen. Bei „nur Webcam“ fehlt der Bildschirm.

  import { editor } from "./state.svelte";
  import { player, type SceneLayer } from "./video.svelte";

  /** CSS-Pixel pro Pixel des Videos. */
  let { zoom }: { zoom: number } = $props();

  let screenEl = $state<HTMLVideoElement | null>(null);
  let webcamEl = $state<HTMLVideoElement | null>(null);
  let systemEl = $state<HTMLAudioElement | null>(null);
  let micEl = $state<HTMLAudioElement | null>(null);
  let overlay = $state<HTMLCanvasElement | null>(null);

  $effect(() => player.attach("screen", screenEl));
  $effect(() => player.attach("webcam", webcamEl));
  $effect(() => player.attach("system", systemEl));
  $effect(() => player.attach("mic", micEl));

  const SOURCES = ["screen", "webcam"] as const;
  const frame = $derived(editor.doc?.artboard.rect ?? { x: 0, y: 0, width: 1, height: 1 });
  const layer = (source: SceneLayer["source"]) => player.scene.layers.findIndex((l) => l.source === source);

  /** Lage einer Spur als CSS (Rahmen = Ausschnitt, Video darin verschoben). */
  function boxStyle(l: SceneLayer, z: number) {
    const [x, y, w, h] = l.rect;
    const shadow = l.shadow > 0 ? 0.05 * Math.min(w, h) + 4 : 0;
    return {
      left: `${x * z}px`,
      top: `${y * z}px`,
      width: `${w * z}px`,
      height: `${h * z}px`,
      radius: `${Math.min(l.radius, w / 2, h / 2) * z}px`,
      opacity: String(l.opacity),
      shadow: shadow ? `0 ${shadow * 0.4 * z}px ${shadow * z}px rgb(0 0 0 / ${l.shadow})` : "none",
    };
  }

  function innerStyle(l: SceneLayer, z: number) {
    const [, , w, h] = l.rect;
    const [cx, cy, cw, ch] = l.crop;
    const width = (w * z) / Math.max(cw, 1e-6);
    const height = (h * z) / Math.max(ch, 1e-6);
    return { left: `${-cx * width}px`, top: `${-cy * height}px`, width: `${width}px`, height: `${height}px` };
  }

  function borderStyle(l: SceneLayer, z: number): string {
    if (!l.border) return "none";
    const [r, g, b, a] = l.border.color;
    return `inset 0 0 0 ${l.border.width * z}px rgb(${r} ${g} ${b} / ${a / 255})`;
  }

  // Zeiger und Klicks zeichnen (gleiche Form wie im Export).
  const OUTER: [number, number][] = [
    [0, 0], [0, 1], [0.25, 0.77], [0.4, 1.08], [0.56, 1.01], [0.42, 0.71], [0.72, 0.71],
  ];
  const INNER: [number, number][] = [
    [0.07, 0.15], [0.07, 0.85], [0.26, 0.67], [0.42, 0.98], [0.47, 0.96], [0.32, 0.64], [0.56, 0.64],
  ];

  $effect(() => {
    const canvas = overlay;
    const z = zoom;
    const layers = player.scene.layers;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    const w = Math.round(frame.width * z * dpr);
    const h = Math.round(frame.height * z * dpr);
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }
    const ctx = canvas.getContext("2d")!;
    ctx.clearRect(0, 0, w, h);
    const s = z * dpr;
    const color = (editor.doc as { video?: { cursor?: { click_color?: { r: number; g: number; b: number } } } } | null)
      ?.video?.cursor?.click_color ?? { r: 229, g: 57, b: 53 };
    for (const l of layers) {
      const [x, y, lw] = l.rect.map((v) => v * s);
      if (l.source === "click") {
        const r = lw / 2 - 1;
        ctx.globalAlpha = l.opacity;
        ctx.strokeStyle = `rgb(${color.r} ${color.g} ${color.b})`;
        ctx.fillStyle = `rgb(${color.r} ${color.g} ${color.b} / 0.18)`;
        ctx.lineWidth = lw * 0.09;
        ctx.beginPath();
        ctx.arc(x + lw / 2, y + lw / 2, Math.max(0, r - ctx.lineWidth / 2), 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
      } else if (l.source === "cursor") {
        // Wie `arrow_image`: Spitze bei 4 %, Pfeil auf 90 % der Höhe.
        const k = (lw * 0.9) / 1.08;
        const ox = x + lw * 0.04;
        const oy = y + lw * 0.04;
        ctx.globalAlpha = l.opacity;
        for (const [poly, fill] of [
          [OUTER, "#fff"],
          [INNER, "#000"],
        ] as const) {
          ctx.beginPath();
          poly.forEach(([px, py], i) => (i ? ctx.lineTo(ox + px * k, oy + py * k) : ctx.moveTo(ox + px * k, oy + py * k)));
          ctx.closePath();
          ctx.fillStyle = fill;
          ctx.fill();
        }
      }
    }
    ctx.globalAlpha = 1;
  });

  function sizes() {
    player.setSizes(
      screenEl?.videoWidth ? [screenEl.videoWidth, screenEl.videoHeight] : null,
      webcamEl?.videoWidth ? [webcamEl.videoWidth, webcamEl.videoHeight] : null,
    );
  }
</script>

{#if player.urls}
  {#each SOURCES as source (source)}
    {@const i = layer(source)}
    {@const l = player.scene.layers[i]}
    {@const url = source === "screen" ? player.urls.screen : player.urls.webcam}
    {#if url}
      {@const box = l ? boxStyle(l, zoom) : null}
      <div
        class="track"
        style:display={l ? "block" : "none"}
        style:z-index={i + 1}
        style:left={box?.left}
        style:top={box?.top}
        style:width={box?.width}
        style:height={box?.height}
        style:border-radius={box?.radius}
        style:opacity={box?.opacity}
        style:box-shadow={box?.shadow}
      >
        <div class="mirror" class:on={l?.mirror}>
          {#if source === "screen"}
            <!-- svelte-ignore a11y_media_has_caption -->
            <video
              bind:this={screenEl}
              src={url}
              muted
              playsinline
              preload="auto"
              onloadedmetadata={sizes}
              style:left={l ? innerStyle(l, zoom).left : null}
              style:top={l ? innerStyle(l, zoom).top : null}
              style:width={l ? innerStyle(l, zoom).width : null}
              style:height={l ? innerStyle(l, zoom).height : null}
            ></video>
          {:else}
            <!-- svelte-ignore a11y_media_has_caption -->
            <video
              bind:this={webcamEl}
              src={url}
              muted
              playsinline
              preload="auto"
              onloadedmetadata={sizes}
              style:left={l ? innerStyle(l, zoom).left : null}
              style:top={l ? innerStyle(l, zoom).top : null}
              style:width={l ? innerStyle(l, zoom).width : null}
              style:height={l ? innerStyle(l, zoom).height : null}
            ></video>
          {/if}
        </div>
        {#if l?.border}
          <div class="border" style:box-shadow={borderStyle(l, zoom)}></div>
        {/if}
      </div>
    {/if}
  {/each}
  <canvas bind:this={overlay} class="pointer" style:z-index={player.scene.layers.length + 2}></canvas>
  {#if player.urls.system}
    <audio bind:this={systemEl} src={player.urls.system} preload="auto"></audio>
  {/if}
  {#if player.urls.mic}
    <audio bind:this={micEl} src={player.urls.mic} preload="auto"></audio>
  {/if}
{/if}

<style>
  .track {
    position: absolute;
    overflow: hidden;
  }
  .mirror {
    position: absolute;
    inset: 0;
  }
  .mirror.on {
    transform: scaleX(-1);
  }
  video {
    position: absolute;
    object-fit: fill;
    max-width: none;
  }
  .border {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    pointer-events: none;
  }
  .pointer {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
</style>
