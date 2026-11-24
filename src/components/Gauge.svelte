<script lang="ts">
  // Segmented 270° temperature ring.
  let { label, temp, fan }: { label: string; temp: number | null; fan: number | null } = $props();

  const id = 'g' + Math.random().toString(36).slice(2, 8);
  const ARC = 292.2; // 3/4 of the ring's circumference (r = 62)
  const FULL = 389.6;

  let hot = $derived(temp !== null && temp >= 85);
  let warm = $derived(temp !== null && temp >= 72);
  let fill = $derived(temp === null ? 0 : Math.max(0, Math.min(1, (temp - 30) / 70)) * ARC);
  let end = $derived(hot ? 'var(--hot)' : 'var(--a1)');
  let stateColor = $derived(hot ? 'var(--hot)' : warm ? 'var(--warm)' : 'var(--a1)');
</script>

<section class="panel gauge" aria-label="{label} temperature">
  <div class="row" style="align-self: stretch; align-items: baseline">
    <span class="label" style="font-size: 17px; letter-spacing: 3px">{label}</span>
    {#if temp !== null}
      <span class="label" style="font-size: 13px; letter-spacing: 2px; color: {stateColor}; text-shadow: 0 0 10px {stateColor}">
        {hot ? 'HOT' : warm ? 'WARM' : 'COOL'}
      </span>
    {/if}
  </div>
  <div class="ring">
    <svg width="176" height="176" viewBox="0 0 160 160" style="transform: rotate(135deg); overflow: visible" aria-hidden="true">
      <defs>
        <linearGradient id={id} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" style="stop-color: var(--a2)" />
          <stop offset="1" style="stop-color: {end}" />
        </linearGradient>
      </defs>
      <circle cx="80" cy="80" r="74" fill="none" style="stroke: var(--border)" stroke-width="1" stroke-dasharray="348.7 465" />
      <circle cx="80" cy="80" r="62" fill="none" style="stroke: color-mix(in srgb, var(--border) 80%, transparent)" stroke-width="11" stroke-dasharray="{ARC} {FULL}" />
      <circle cx="80" cy="80" r="62" fill="none" stroke="url(#{id})" stroke-width="11" stroke-dasharray="{fill} {FULL}"
        style="filter: drop-shadow(0 0 6px {end}); transition: stroke-dasharray .6s ease" />
      <circle cx="80" cy="80" r="62" fill="none" stroke="#000" stroke-opacity=".55" stroke-width="12" stroke-dasharray="1.2 8.53" />
    </svg>
    <div class="readout">
      <div class="num" style="font-size: 38px; text-shadow: 0 0 18px {end}">{temp ?? '--'}°</div>
      {#if fan !== null}
        <div class="label muted" style="font-size: 13px; letter-spacing: 2px; margin-top: 2px">FAN {fan}%</div>
      {/if}
    </div>
  </div>
</section>

<style>
  .gauge { display: flex; flex-direction: column; align-items: center; gap: 4px; padding: 18px 20px; }
  .ring { position: relative; width: 176px; height: 158px; }
  .readout { position: absolute; inset: 0 0 18px; display: flex; flex-direction: column; align-items: center; justify-content: center; }
</style>
