<script lang="ts">
  import FanCurve from '../components/FanCurve.svelte';
  import Switch from '../components/Switch.svelte';
  import { api, type FanCurve as Curve, type FanPreset, type Gpu, type Status } from '../lib/api';
  import { clearPageActions, pageActions } from '../lib/pageActions.svelte.js';

  let { status, busy, run }: {
    status: Status;
    busy: boolean;
    run: (action: () => Promise<unknown>, success: string) => Promise<void>;
  } = $props();

  const PRESETS: { id: FanPreset; label: string }[] = [
    { id: 'auto', label: 'Auto' },
    { id: 'quiet', label: 'Quiet' },
    { id: 'cool', label: 'Cool' },
    { id: 'custom', label: 'Custom' }
  ];
  const HINTS: Record<FanPreset, string> = {
    auto: "The laptop's own fan curve, following the performance mode.",
    quiet: 'Factory curve with slower fans below 70°C. Hot temperatures are unchanged.',
    cool: 'Factory curve with every point 10% faster.',
    custom: 'Drag a point sideways for temperature, up or down for fan speed, then press Apply.'
  };

  let preset = $derived(status.state.fanPreset);
  let limits = $derived(status.powerLimits);

  // Edits stay on screen until Apply in the header. While nothing has been
  // changed, the controls follow what the service reports.
  let fanDraft = $state<{ cpu: Curve; gpu: Curve } | null>(null);
  let test = $derived(status.supported.fanTest ?? { available: false, running: false, phase: null, ok: null, message: null });
  let fanDirty = $state(false);

  let pl1 = $state(0);
  let pl2 = $state(0);
  let turbo = $state(true);
  let cpuDirty = $state(false);
  $effect(() => {
    if (!cpuDirty) {
      pl1 = Math.round(status.state.pl1W ?? 0);
      pl2 = Math.round(status.state.pl2W ?? 0);
      turbo = status.state.turbo === true;
    }
  });

  // GPUs: one draft per GPU, keyed by its id, until Apply. A draft is
  // dropped as soon as it matches what the GPU already has.
  type GpuDraft = { clock: number; power: number; level: string };
  let gpus = $derived(status.gpus);
  let selectedId = $state<string | null>(null);
  let sel = $derived(gpus.find((g) => g.id === selectedId) ?? gpus[0] ?? null);
  let drafts = $state<Record<string, GpuDraft>>({});
  let boostDraft = $state<boolean | null>(null);

  function liveDraft(g: Gpu): GpuDraft {
    return {
      clock: g.clock ? (g.clock.limitMhz ?? g.clock.maxMhz) : 0,
      power: g.power ? (g.power.limitW ?? g.power.defaultW) : 0,
      level: g.level?.current ?? 'auto'
    };
  }
  function gpuDraft(g: Gpu): GpuDraft {
    return drafts[g.id] ?? liveDraft(g);
  }
  function editGpu(g: Gpu, patch: Partial<GpuDraft>) {
    const next = { ...gpuDraft(g), ...patch };
    const live = liveDraft(g);
    if (next.clock === live.clock && next.power === live.power && next.level === live.level) delete drafts[g.id];
    else drafts[g.id] = next;
  }
  function gpuChanges(g: Gpu): { clock?: number; power?: number; level?: string } {
    const d = drafts[g.id];
    if (!d) return {};
    const live = liveDraft(g);
    const out: { clock?: number; power?: number; level?: string } = {};
    if (g.clock && d.clock !== live.clock) out.clock = d.clock >= g.clock.maxMhz ? 0 : d.clock;
    if (g.power && d.power !== live.power) out.power = d.power === g.power.defaultW ? 0 : d.power;
    if (g.level && d.level !== live.level) out.level = d.level;
    return out;
  }
  let gpuPending = $derived(gpus.some((g) => Object.keys(gpuChanges(g)).length > 0));
  let boostChanged = $derived(status.dynamicBoost !== null && boostDraft !== null && boostDraft !== status.dynamicBoost.on);
  let d = $derived(sel ? gpuDraft(sel) : null);
  let selPending = $derived(sel ? Object.keys(gpuChanges(sel)).length > 0 : false);

  const LEVELS: Record<string, string> = { auto: 'Auto', low: 'Power saving', high: 'Maximum' };
  function shortName(name: string): string {
    return name.replace(/^(NVIDIA GeForce|NVIDIA|AMD|Intel\(R\)|Intel)\s+/, '');
  }
  function liveLine(g: Gpu): string {
    if (!g.awake) return 'Sleeping';
    return [
      g.powerW !== null ? `${Math.round(g.powerW)} W` : null,
      g.clockMhz !== null ? `${g.clockMhz.toLocaleString('en-US')} MHz` : null,
      g.tempC !== null ? `${g.tempC}°C` : null
    ].filter(Boolean).join(' · ');
  }

  let powerChanged = $derived(
    status.supported.cpuPower && (pl1 !== Math.round(status.state.pl1W ?? 0) || pl2 !== Math.round(status.state.pl2W ?? 0))
  );
  let turboChanged = $derived(status.supported.turbo && turbo !== (status.state.turbo === true));
  let fanChanged = $derived(fanDirty && preset === 'custom' && fanDraft !== null);

  function undo() {
    fanDirty = false;
    cpuDirty = false;
    drafts = {};
    boostDraft = null;
  }

  async function apply() {
    const fan = fanChanged ? $state.snapshot(fanDraft) : null;
    const power = powerChanged ? [pl1, pl2] as const : null;
    const turboOn = turboChanged ? turbo : null;
    const gpuSteps = gpus.map((g) => ({ g, c: gpuChanges(g) })).filter((x) => Object.keys(x.c).length > 0);
    const boostOn = boostChanged ? boostDraft : null;
    const parts: string[] = [];
    if (fan) parts.push('fan curve');
    if (power) parts.push(`CPU power ${power[0]} W / ${power[1]} W`);
    if (turboOn !== null) parts.push(`Turbo Boost ${turboOn ? 'on' : 'off'}`);
    for (const { g, c } of gpuSteps) {
      const name = gpus.length > 1 ? `${shortName(g.name)} ` : 'GPU ';
      if (c.clock !== undefined) parts.push(c.clock === 0 ? `${name}clock unlimited` : `${name}clock ${c.clock} MHz`);
      if (c.power !== undefined) parts.push(c.power === 0 ? `${name}power default` : `${name}power ${c.power} W`);
      if (c.level !== undefined) parts.push(`${name}${LEVELS[c.level] ?? c.level}`);
    }
    if (boostOn !== null) parts.push(`Dynamic Boost ${boostOn ? 'on' : 'off'}`);
    if (parts.length === 0) return undo();
    await run(async () => {
      if (fan) await api.setCustomFanCurve(fan.cpu, fan.gpu);
      if (power) await api.setCpuPower(power[0], power[1]);
      if (turboOn !== null) await api.setTurbo(turboOn);
      for (const { g, c } of gpuSteps) {
        if (c.clock !== undefined) await api.setGpuClockLimit(g.id, c.clock);
        if (c.power !== undefined) await api.setGpuPowerLimit(g.id, c.power);
        if (c.level !== undefined) await api.setGpuLevel(g.id, c.level);
      }
      if (boostOn !== null) await api.setDynamicBoost(boostOn);
    }, `Applied: ${parts.join(', ')}`);
    undo();
  }

  function reset() {
    undo();
    run(() => api.resetAdvanced(), 'Advanced settings reset to laptop defaults');
  }

  // Dragged back to where it started: nothing left to apply.
  $effect(() => {
    if (cpuDirty && !powerChanged && !turboChanged) cpuDirty = false;
  });

  // Hand Apply / Undo / Reset to the header while this page is open.
  $effect(() => {
    pageActions.pending = fanChanged || powerChanged || turboChanged || gpuPending || boostChanged;
    pageActions.apply = apply;
    pageActions.undo = undo;
    pageActions.reset = reset;
    return clearPageActions;
  });
</script>

<section class="panel section" aria-labelledby="fan-h">
  <div class="row" style="flex-wrap: wrap">
    <div>
      <h2 id="fan-h" class="label" style="font-size: 21px">Fan curve</h2>
      <div class="small muted">
        {status.supported.fanCurves ? (status.fanCurve?.gpu === null ? `${HINTS[preset]} One curve drives all fans on this laptop.` : HINTS[preset])
          : test.available || test.running ? 'Not verified on this model yet.' : status.supported.fanCurveReason ?? 'Fan curves are unavailable.'}
      </div>
    </div>
    {#if status.supported.fanCurves}
      <div class="seg" role="group" aria-label="Fan curve">
        {#each PRESETS as p (p.id)}
          <button type="button" class:active={preset === p.id} aria-pressed={preset === p.id} disabled={busy}
            onclick={() => { if (preset !== p.id) { fanDirty = false; run(() => api.setFanPreset(p.id), `${p.label} fan curve applied`); } }}>{p.label}</button>
        {/each}
      </div>
    {/if}
  </div>
  {#if status.supported.fanCurves && status.fanCurve}
    <FanCurve curve={status.fanCurve} editable={preset === 'custom'} bind:draft={fanDraft} bind:dirty={fanDirty} />
  {:else if test.available || test.message}
    <div class="fantest" class:running={test.running}>
      <div class="ft-text">
        {#if test.running}
          <div class="label accent">Testing fans{test.phase === 'gpu' ? ' (GPU curve)' : test.phase === 'settle' ? ': letting the fans settle' : ' (CPU curve)'}…</div>
          <div class="small muted">The fans will get loud for a moment. Everything goes back to how it was afterwards.</div>
        {:else if test.message && test.ok === false}
          <div class="label">Fan test didn't pass</div>
          <div class="small muted">{test.message}</div>
        {:else}
          <div class="label">Unlock fan curves on this laptop</div>
          <div class="small muted">
            Your model uses MSI's usual fan-table layout but hasn't been verified yet. A short test (under a minute) raises
            the fan curve to full speed, checks the fans respond, then puts everything back. Run it while the laptop is idle.
          </div>
        {/if}
      </div>
      {#if test.running}
        <button type="button" class="btn" disabled={busy} onclick={() => run(() => api.cancelFanTest(), 'Fan test cancelled')}>Cancel</button>
      {:else if test.available}
        <button type="button" class="btn solid" disabled={busy} onclick={() => run(() => api.startFanTest(), 'Fan test started')}>
          {test.ok === false ? 'Try again' : 'Run fan test'}</button>
      {/if}
    </div>
  {/if}
</section>

<div class="grid-2">
  {#if status.supported.cpuPower && limits}
    <section class="panel" style="display: flex; flex-direction: column; gap: 14px" aria-labelledby="pl-h">
      <div class="row">
        <h2 id="pl-h" class="label" style="font-size: 21px">CPU power</h2>
        {#if powerChanged}<span class="small accent">Not applied yet</span>
        {:else if !status.state.powerCustom}<span class="small muted">Laptop default</span>{/if}
      </div>
      <div style="display: flex; flex-direction: column; gap: 6px">
        <div class="row" style="align-items: baseline"><label for="pl1">Sustained</label><span class="num accent">{pl1} W</span></div>
        <input id="pl1" type="range" min={limits.pl1Min} max={limits.pl1Max} step="1" bind:value={pl1} disabled={busy}
          oninput={() => { cpuDirty = true; if (pl2 < pl1) pl2 = pl1; }} />
      </div>
      <div style="display: flex; flex-direction: column; gap: 6px">
        <div class="row" style="align-items: baseline"><label for="pl2">Short burst</label><span class="num accent">{pl2} W</span></div>
        <input id="pl2" type="range" min={limits.pl2Min} max={limits.pl2Max} step="1" bind:value={pl2} disabled={busy}
          oninput={() => { cpuDirty = true; if (pl1 > pl2) pl1 = pl2; }} />
      </div>
      <div class="small muted">The sliders stop at this laptop's own defaults ({limits.pl1Max} W / {limits.pl2Max} W).</div>
    </section>
  {/if}

  <div class="stack">
    {#if status.supported.turbo}
      <section class="panel row">
        <div>
          <div class="label" style="font-size: 20px">Turbo Boost</div>
          <div class="small" class:muted={!turboChanged} class:accent={turboChanged}>
            {turboChanged ? 'Not applied yet' : 'Lets the CPU run above its base clock'}
          </div>
        </div>
        <Switch checked={turbo} label="Turbo Boost" disabled={busy} onchange={(on) => { turbo = on; cpuDirty = true; }} />
      </section>
    {/if}
    <section class="panel" style="display: flex; flex-direction: column; gap: 12px" aria-labelledby="gpu-h" title={sel?.name}>
      <div class="row">
        <h2 id="gpu-h" class="label" style="font-size: 20px">GPU</h2>
        {#if selPending || boostChanged}<span class="small accent">Not applied yet</span>
        {:else if sel}<span class="small muted">{liveLine(sel)}</span>{/if}
      </div>
      {#if gpus.length > 1}
        <div class="seg" role="group" aria-label="Which GPU">
          {#each gpus as g (g.id)}
            <button type="button" class:active={sel?.id === g.id} aria-pressed={sel?.id === g.id}
              onclick={() => (selectedId = g.id)}>{shortName(g.name)}</button>
          {/each}
        </div>
      {/if}
      {#if !sel || !d}
        <div class="small muted">No GPU found.</div>
      {:else}
        {#if sel.power}
          <div style="display: flex; flex-direction: column; gap: 6px">
            <div class="row" style="align-items: baseline">
              <label for="gpw">Power limit</label>
              <span class="num accent">{d.power} W{d.power === sel.power.defaultW ? ' (default)' : ''}</span>
            </div>
            <input id="gpw" type="range" min={sel.power.minW} max={sel.power.maxW} step="1" value={d.power} disabled={busy}
              oninput={(e) => editGpu(sel!, { power: Number(e.currentTarget.value) })} />
          </div>
        {/if}
        {#if sel.clock}
          <div style="display: flex; flex-direction: column; gap: 6px">
            <div class="row" style="align-items: baseline">
              <label for="gclk">Max clock</label>
              <span class="num accent">{d.clock >= sel.clock.maxMhz ? 'No limit' : `${d.clock.toLocaleString('en-US')} MHz`}</span>
            </div>
            <input id="gclk" type="range" min={sel.clock.minMhz} max={sel.clock.maxMhz} step={sel.clock.stepMhz} value={d.clock} disabled={busy}
              oninput={(e) => editGpu(sel!, { clock: Number(e.currentTarget.value) })} />
          </div>
        {/if}
        {#if sel.level}
          <div class="row">
            <span style="font-weight: 600; font-size: 15px">Clock mode</span>
            <div class="seg" role="group" aria-label="GPU clock mode">
              {#each sel.level.options as l (l)}
                <button type="button" class:active={d.level === l} aria-pressed={d.level === l} disabled={busy}
                  onclick={() => editGpu(sel!, { level: l })}>{LEVELS[l] ?? l}</button>
              {/each}
            </div>
          </div>
        {/if}
        {#if status.dynamicBoost && sel.vendor === 'nvidia'}
          <div class="row">
            <div>
              <div style="font-weight: 600; font-size: 15px">Dynamic Boost</div>
              <div class="small muted">Gives the GPU extra power from the CPU's budget under load</div>
            </div>
            <Switch checked={boostDraft ?? status.dynamicBoost.on} label="Dynamic Boost" disabled={busy}
              onchange={(on) => (boostDraft = on === status.dynamicBoost?.on ? null : on)} />
          </div>
        {/if}
        {#if !sel.power && !sel.clock && !sel.level && !(status.dynamicBoost && sel.vendor === 'nvidia')}
          <div class="small muted">
            {sel.awake ? `The ${sel.driver} driver doesn't offer adjustable GPU settings here.` : 'Controls appear once the GPU has been used.'}
          </div>
        {/if}
      {/if}
    </section>
  </div>
</div>

<style>
  .fantest { display: flex; align-items: center; gap: 18px; padding: 16px 18px; margin-top: 12px; border: 1px dashed color-mix(in srgb, var(--a1) 40%, transparent); border-radius: 10px; }
  .fantest.running { border-style: solid; box-shadow: 0 0 18px color-mix(in srgb, var(--a1) 25%, transparent); }
  .ft-text { flex: 1; display: flex; flex-direction: column; gap: 4px; }
</style>
