<script lang="ts">
  // One fan card: a fan that spins at a rate tied to the real RPM.
  let { label, rpm, peak }: { label: string; rpm: number; peak: number } = $props();

  // Real fans turn far too fast to draw (6000 RPM = 100 turns a second), so the
  // drawing uses a calm, square-root scale: about 0.35 turns a second at
  // 1000 RPM, 0.7 at 4000 and 0.85 at 6000, never more than one turn a second.
  // Faster fans still visibly spin faster, without the strobing look.
  const turnsFor = (rpm: number) => Math.min(1, 0.35 * Math.sqrt(Math.max(0, rpm) / 1000));
  const BLADES = Array.from({ length: 7 }, (_, i) => (360 / 7) * i);

  let angle = $state(0);
  let spinning = $derived(rpm > 0);
  let fill = $derived(Math.min(100, (rpm / Math.max(peak, 1)) * 100));

  $effect(() => {
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    let speed = 0; // turns per second, eased toward the target so changes look smooth
    let last = performance.now();
    let frame = requestAnimationFrame(function tick(now) {
      const dt = Math.min(0.1, (now - last) / 1000);
      last = now;
      const target = turnsFor(rpm);
      speed += (target - speed) * Math.min(1, dt * 1.5);
      angle = (angle + speed * 360 * dt) % 360;
      frame = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(frame);
  });
</script>

<section class="panel fan" aria-label="{label}: {rpm} RPM">
  <svg class="rotor" class:on={spinning} width="68" height="68" viewBox="-50 -50 100 100" aria-hidden="true">
    <circle r="47" fill="none" style="stroke: var(--border)" stroke-width="2" />
    <circle r="42" fill="none" style="stroke: color-mix(in srgb, var(--a1) 25%, transparent)" stroke-width="1" stroke-dasharray="2 5" />
    <g transform="rotate({angle})">
      {#each BLADES as turn (turn)}
        <path transform="rotate({turn})" d="M2 -9 C 10 -24, 26 -34, 38 -20 C 30 -14, 18 -9, 8 -4 Z" class="blade" />
      {/each}
      <circle r="11" class="hub" />
      <circle r="4" style="fill: var(--bg)" />
    </g>
  </svg>
  <div class="info">
    <div class="label" style="font-size: 17px; letter-spacing: 2px">{label}</div>
    <div class="num rpm" class:accent={spinning}>{rpm.toLocaleString('en-US')}<span class="unit">RPM</span></div>
    <div class="bar"><div class="bar-fill" style="width: {fill}%"></div></div>
    <div class="small muted">{spinning ? 'Spinning' : 'Stopped'}</div>
  </div>
</section>

<style>
  .fan { display: flex; align-items: center; gap: 14px; padding: 16px 18px; }
  .rotor { flex-shrink: 0; }
  .blade { fill: color-mix(in srgb, var(--muted) 55%, transparent); transition: fill .4s; }
  .hub { fill: color-mix(in srgb, var(--muted) 70%, transparent); }
  .rotor.on .blade { fill: color-mix(in srgb, var(--a1) 80%, var(--a2)); }
  .rotor.on .hub { fill: var(--a1); }
  .rotor.on { filter: drop-shadow(0 0 8px var(--glow)); }
  /* The info column is a size container, so the RPM text scales with the card
     instead of overflowing when four or more fans share a row. */
  .info { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 6px; container-type: inline-size; }
  .rpm { font-size: clamp(14px, 12cqi, 24px); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .unit { font-size: .5em; margin-left: .4em; color: var(--muted); text-shadow: none; }
</style>
