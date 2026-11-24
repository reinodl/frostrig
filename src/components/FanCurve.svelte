<script lang="ts">
  import type { FanCurve } from '../lib/api';

  type Pair = { cpu: FanCurve; gpu: FanCurve };
  // The edited curve and whether it differs are bound to the parent, which
  // applies them from the page-level Apply button.
  let { curve, editable, draft = $bindable(null), dirty = $bindable(false) }: {
    /** gpu is null on single-curve boards: one curve drives every fan. */
    curve: { cpu: FanCurve; gpu: FanCurve | null };
    editable: boolean;
    draft?: Pair | null;
    dirty?: boolean;
  } = $props();

  const W = 600, H = 220, T_MIN = 30, T_MAX = 100;
  let part = $state<'cpu' | 'gpu'>('cpu');
  let dragging = $state<number | null>(null);
  let plot: HTMLDivElement | undefined = $state();

  const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

  // Custom curves follow the service's rules: 30–100 °C rising, 30–100% never
  // falling, last point 100 °C at 100%.
  function normalize(c: FanCurve): FanCurve {
    const n = c.temps.length;
    const temps = c.temps.map((t, i) => clamp(Math.round(t), T_MIN + i, T_MAX - (n - 1 - i)));
    for (let i = 1; i < n; i++) temps[i] = Math.max(temps[i], temps[i - 1] + 1);
    temps[n - 1] = 100;
    const speeds = c.speeds.map((s) => clamp(Math.round(s), 30, 100));
    for (let i = 1; i < n; i++) speeds[i] = Math.max(speeds[i], speeds[i - 1]);
    speeds[n - 1] = 100;
    return { temps, speeds };
  }

  let single = $derived(curve.gpu === null);
  let pair = $derived<Pair>({ cpu: curve.cpu, gpu: curve.gpu ?? curve.cpu });
  $effect(() => { if (single) part = 'cpu'; });

  $effect(() => {
    if (editable && !dirty) draft = { cpu: normalize(pair.cpu), gpu: normalize(pair.gpu) };
    if (!editable) { draft = null; dirty = false; }
  });

  let shown = $derived(editable && draft ? draft[part] : pair[part]);
  let yMax = $derived(Math.max(100, ...shown.speeds));
  let pts = $derived(shown.temps.map((t, i) => ({
    x: ((clamp(t, T_MIN, T_MAX) - T_MIN) / (T_MAX - T_MIN)) * 100,
    y: 100 - (shown.speeds[i] / yMax) * 100,
    t, s: shown.speeds[i], last: i === shown.temps.length - 1
  })));
  let line = $derived(pts.map((p) => `${(p.x * W) / 100},${(p.y * H) / 100}`).join(' '));
  let area = $derived(`0,${H} ${line} ${W},${H}`);

  function setSpeed(i: number, value: number) {
    if (!draft || i === shown.speeds.length - 1) return;
    const c = draft[part];
    const lo = i === 0 ? 30 : c.speeds[i - 1];
    const hi = c.speeds[i + 1];
    c.speeds[i] = clamp(Math.round(value), Math.max(30, lo), Math.min(100, hi));
    dirty = true;
  }

  // Each point stays at least 1 °C away from its neighbours, so the order
  // never changes; the last point is fixed at 100 °C.
  function setTemp(i: number, value: number) {
    if (!draft || i === shown.temps.length - 1) return;
    const c = draft[part];
    const lo = i === 0 ? T_MIN : c.temps[i - 1] + 1;
    const hi = c.temps[i + 1] - 1;
    c.temps[i] = clamp(Math.round(value), lo, hi);
    dirty = true;
  }

  function fromPointer(e: PointerEvent): { t: number; s: number } {
    const r = plot!.getBoundingClientRect();
    return {
      t: T_MIN + ((e.clientX - r.left) / r.width) * (T_MAX - T_MIN),
      s: (1 - (e.clientY - r.top) / r.height) * yMax
    };
  }

  function down(i: number, e: PointerEvent) {
    if (!editable || pts[i].last) return;
    dragging = i;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function move(e: PointerEvent) {
    if (dragging === null) return;
    const p = fromPointer(e);
    setTemp(dragging, p.t);
    setSpeed(dragging, p.s);
  }
  function up() {
    dragging = null;
  }
  function key(i: number, e: KeyboardEvent) {
    if (!editable || pts[i].last) return;
    const step = e.shiftKey ? 10 : e.key.startsWith('ArrowL') || e.key.startsWith('ArrowR') ? 1 : 2;
    const moves: Record<string, () => void> = {
      ArrowUp: () => setSpeed(i, shown.speeds[i] + step),
      ArrowDown: () => setSpeed(i, shown.speeds[i] - step),
      ArrowRight: () => setTemp(i, shown.temps[i] + step),
      ArrowLeft: () => setTemp(i, shown.temps[i] - step)
    };
    if (moves[e.key]) { moves[e.key](); e.preventDefault(); }
  }

</script>

<div class="curve">
  <div class="plot-wrap">
    <div class="y-axis label muted"><span>{yMax}%</span><span>{Math.round(yMax / 2)}%</span><span>0%</span></div>
    <!-- Dragging is handled here; each point is also a keyboard-operable button. -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="plot" bind:this={plot} onpointermove={move} onpointerup={up} onpointercancel={up}>
      <svg width="100%" height="100%" viewBox="0 0 {W} {H}" preserveAspectRatio="none" aria-hidden="true">
        <defs>
          <linearGradient id="fanFill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" style="stop-color: var(--a1); stop-opacity: .35" />
            <stop offset="1" style="stop-color: var(--a1); stop-opacity: 0" />
          </linearGradient>
          <linearGradient id="fanStroke" x1="0" y1="0" x2="1" y2="0">
            <stop offset="0" style="stop-color: var(--a2)" />
            <stop offset="1" style="stop-color: var(--a1)" />
          </linearGradient>
        </defs>
        <path d="M0 55H600M0 110H600M0 165H600M85.7 0V220M171.4 0V220M257.1 0V220M342.9 0V220M428.6 0V220M514.3 0V220"
          style="stroke: color-mix(in srgb, var(--a1) 10%, transparent)" stroke-width="1" vector-effect="non-scaling-stroke" />
        <polygon points={area} fill="url(#fanFill)" />
        <polyline points={line} fill="none" stroke="url(#fanStroke)" stroke-width="3" stroke-linejoin="round"
          vector-effect="non-scaling-stroke" style="filter: drop-shadow(0 0 6px var(--a1))" />
      </svg>
      {#if single}
        <div class="seg which" role="note" title="This laptop has one fan curve for all its fans."><button type="button" class="active" disabled>All fans</button></div>
      {:else}
        <div class="seg which" role="group" aria-label="Which fan">
          <button type="button" class:active={part === 'cpu'} aria-pressed={part === 'cpu'} onclick={() => (part = 'cpu')}>CPU fan</button>
          <button type="button" class:active={part === 'gpu'} aria-pressed={part === 'gpu'} onclick={() => (part = 'gpu')}>GPU fan</button>
        </div>
      {/if}
      {#each pts as p, i (i)}
        {#if editable && !p.last}
          <button type="button" class="pt edit" style="left: {p.x}%; top: {p.y}%"
            aria-label="{p.t}°C: {p.s}% fan. Left and right change the temperature, up and down the fan speed."
            onpointerdown={(e) => down(i, e)} onkeydown={(e) => key(i, e)}></button>
        {:else}
          <span class="pt" style="left: {p.x}%; top: {p.y}%" title="{p.t}°C: {p.s}%"></span>
        {/if}
        {#if dragging === i}
          <span class="tip num" style="left: {p.x}%; top: {p.y}%">{p.t}° · {p.s}%</span>
        {/if}
      {/each}
    </div>
  </div>
  <div class="x-axis label muted"><span>30°C</span><span>40°</span><span>50°</span><span>60°</span><span>70°</span><span>80°</span><span>90°</span><span>100°C</span></div>
</div>

<style>
  .curve { display: flex; flex-direction: column; gap: 8px; }
  /* Sits in the chart's top-left corner, where low-temperature points never go. */
  .which { position: absolute; top: 8px; left: 8px; z-index: 1; padding: 3px; }
  .which button { padding: 5px 12px; font-size: 13px; }
  .plot-wrap { display: flex; gap: 10px; }
  .y-axis { display: flex; flex-direction: column; justify-content: space-between; width: 40px; text-align: right; font-size: 12px; }
  .plot {
    flex: 1; position: relative; height: 200px; border-radius: 4px; touch-action: none;
    background: color-mix(in srgb, var(--bg) 70%, transparent); border: 1px solid var(--border);
    box-shadow: inset 0 0 30px color-mix(in srgb, var(--a1) 6%, transparent);
  }
  .plot svg { position: absolute; inset: 0; overflow: visible; }
  .pt {
    position: absolute; width: 10px; height: 10px; margin: -5px 0 0 -5px; transform: rotate(45deg);
    background: var(--a1); box-shadow: 0 0 10px var(--a1);
  }
  .pt.edit { width: 16px; height: 16px; margin: -8px 0 0 -8px; background: var(--bg); border: 3px solid var(--a1); cursor: move; }
  .pt.edit:hover, .pt.edit:focus-visible { background: var(--a1); }
  .tip {
    position: absolute; transform: translate(-50%, -150%); padding: 4px 8px; border-radius: 3px; font-size: 12px; white-space: nowrap;
    background: var(--panel); border: 1px solid var(--a1); pointer-events: none;
  }
  .x-axis { display: flex; justify-content: space-between; margin-left: 50px; font-size: 12px; }
</style>
