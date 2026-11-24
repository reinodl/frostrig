<script module lang="ts">
  // Remembered across tab switches. Some drivers expose more fan channels than
  // the laptop has; those always read 0, so once any fan has spun, only fans
  // that have spun at least once are shown.
  const everSpun = new Set<number>();
  let peakRpm = 4000;
</script>

<script lang="ts">
  import FanSpinner from '../components/FanSpinner.svelte';
  import Icon from '../components/Icon.svelte';
  import Gauge from '../components/Gauge.svelte';
  import Switch from '../components/Switch.svelte';
  import { api, type ModeId, type Status } from '../lib/api';

  let { status, busy, run, goBattery }: {
    status: Status;
    busy: boolean;
    run: (action: () => Promise<unknown>, success: string) => void;
    goBattery: () => void;
  } = $props();

  const MODES: { id: ModeId; label: string; desc: string; icon: 'leaf' | 'scale' | 'gauge' | 'flame' }[] = [
    { id: 'silent', label: 'Silent', desc: 'Quietest fans. Lower power for browsing and calls.', icon: 'leaf' },
    { id: 'balanced', label: 'Balanced', desc: 'Everyday use. Fans react to load.', icon: 'scale' },
    { id: 'performance', label: 'Performance', desc: 'Higher power for gaming and builds.', icon: 'gauge' },
    { id: 'extreme', label: 'Extreme', desc: 'Maximum power. Loud under load; best plugged in.', icon: 'flame' }
  ];

  let modes = $derived(MODES.filter((m) => status.supported.modes.includes(m.id)));
  let boost = $derived(status.state.maxFans === true);
  let b = $derived(status.battery);
  // Max Fans and battery sit beside the gauges; desktops may have neither.
  let sideColumn = $derived(status.supported.maxFans || status.battery.present);
  let fans = $derived.by(() => {
    const all = status.sensors.fans.map((f, i) => ({ ...f, i }));
    for (const f of all) {
      if (f.rpm > 0) everSpun.add(f.i);
      peakRpm = Math.max(peakRpm, f.rpm);
    }
    return everSpun.size === 0 ? all : all.filter((f) => everSpun.has(f.i));
  });
  let peak = $derived(Math.max(peakRpm, ...fans.map((f) => f.rpm)));
  let batteryLine = $derived(
    b.acOnline === false ? 'On battery'
      : b.chargeLimit !== null && b.chargeLimit < 100 ? `Plugged in · stops at ${b.chargeLimit}%`
      : 'Plugged in'
  );
</script>

{#if modes.length > 0}
  <section class="section" aria-labelledby="mode-h">
    <h2 id="mode-h" class="section-label">Performance mode</h2>
    <div class={modes.length === 4 ? 'grid-4' : 'grid-3'}>
      {#each modes as m, i (m.id)}
        {@const on = status.state.mode === m.id}
        <button type="button" class="panel pick" class:selected={on} aria-pressed={on} disabled={busy && !on}
          style="min-height: 176px"
          onclick={() => !on && run(() => api.setMode(m.id), `${m.label} mode on`)}>
          <span class="topline"></span>
          <div class="row" style="align-self: stretch; align-items: flex-start">
            <div class="icon-box"><Icon name={m.icon} size={24} /></div>
            <span class="num" style="font-size: 13px" class:accent={on} class:muted={!on}>0{i + 1}</span>
          </div>
          <div class="label" style="font-size: 23px">{m.label}</div>
          <div class="small muted">{m.desc}</div>
        </button>
      {/each}
    </div>
  </section>
{:else}
  <section class="panel">
    <div class="label" style="font-size: 20px">Performance modes unavailable</div>
    <p class="small muted" style="margin: 6px 0 0">
      {status.device.msiEc
        ? "The msi-ec driver doesn't offer performance modes on this laptop."
        : "This machine doesn't offer performance modes to Linux. On MSI laptops, install the msi-ec driver; on other laptops, installing power-profiles-daemon usually adds them."}
    </p>
  </section>
{/if}

<div class={sideColumn ? 'grid-3' : 'grid-2'}>
  <Gauge label="CPU" temp={status.sensors.cpuTemp} fan={status.sensors.cpuFan} />
  <Gauge label="GPU" temp={status.sensors.gpuTemp} fan={status.sensors.gpuFan} />
  {#if sideColumn}
  <div class="stack">
    {#if status.supported.maxFans}
      <section class="panel" class:lit={boost} style="flex: 1; display: flex; flex-direction: column; justify-content: space-between; gap: 12px; padding: 18px 20px">
        <div class="row">
          <div>
            <div class="label" style="font-size: 20px">Max Fans</div>
            <div class="small muted">All fans at full speed</div>
          </div>
          <Switch checked={boost} label="Max Fans" disabled={busy}
            onchange={(on) => run(() => api.setMaxFans(on), on ? 'Max Fans on' : 'Max Fans off')} />
        </div>
        <div class="small" style="display: flex; align-items: center; gap: 8px; color: {boost ? 'var(--a1)' : 'var(--muted)'}">
          <span class:spin={boost} style="display: inline-flex"><Icon name="fan" size={16} /></span>
          {boost ? 'On: fans at maximum' : 'Off: fans follow the mode'}
        </div>
      </section>
    {/if}
    {#if b.present}
    <button type="button" class="panel" style="flex: 1; display: flex; flex-direction: column; justify-content: space-between; gap: 12px; padding: 18px 20px" onclick={goBattery}>
      <div class="row" style="align-self: stretch">
        <span class="label" style="font-size: 20px">Battery</span>
        <span class="num accent" style="font-size: 22px">{b.percent ?? '--'}%</span>
      </div>
      <div class="bar" style="align-self: stretch"><div class="bar-fill" style="width: {b.percent ?? 0}%"></div></div>
      <span class="small muted">{batteryLine}</span>
    </button>
    {/if}
  </div>
  {/if}
</div>

{#if fans.length > 0}
  <section class="section" aria-labelledby="fans-h">
    <h2 id="fans-h" class="section-label">Fans</h2>
    <div class="fan-grid">
      {#each fans as f (f.i)}
        <FanSpinner label={f.label} rpm={f.rpm} {peak} />
      {/each}
    </div>
  </section>
{/if}

<style>
  .fan-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(230px, 1fr)); gap: 14px; }
</style>
