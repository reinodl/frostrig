<script lang="ts">
  import Switch from '../components/Switch.svelte';
  import { api, type Status } from '../lib/api';

  let { status, busy, run }: {
    status: Status;
    busy: boolean;
    run: (action: () => Promise<unknown>, success: string) => void;
  } = $props();

  const LIMITS = [
    { value: 100, label: 'Best for mobility', desc: 'Charges to full. Pick this before a day away from the charger.' },
    { value: 80, label: 'Balanced', desc: 'Stops at 80%. Good if you unplug now and then.' },
    { value: 60, label: 'Best for battery', desc: 'Stops at 60%. Longest battery lifespan when you are mostly plugged in.' }
  ];

  let b = $derived(status.battery);
  let statusLine = $derived.by(() => {
    if (b.acOnline === false) return b.status === 'Discharging' ? 'On battery' : 'Not plugged in';
    const limit = b.chargeLimit ?? 100;
    if (b.status === 'Charging') return limit < 100 ? `Plugged in, charging until ${limit}%` : 'Plugged in, charging to 100%';
    if (b.status === 'Full') return 'Plugged in, fully charged';
    return limit < 100 ? `Plugged in, holding at the ${limit}% limit` : 'Plugged in';
  });
</script>

<section class="panel" style="display: flex; flex-wrap: wrap; align-items: center; gap: 28px; padding: 24px 26px">
  <div class="num" style="font-size: 54px; line-height: 1; text-shadow: 0 0 24px var(--glow)">{b.percent ?? '--'}%</div>
  <div style="flex: 1 1 260px; display: flex; flex-direction: column; gap: 12px">
    <div class="bar big"><div class="bar-fill" style="width: {b.percent ?? 0}%"></div></div>
    <div class="muted" style="font-size: 14px">{statusLine}</div>
  </div>
</section>

<section class="section" aria-labelledby="bat-h">
  <h2 id="bat-h" class="section-label">Charging</h2>
  {#if status.supported.chargeLimit}
    <div class="grid-3">
      {#each LIMITS as l (l.value)}
        {@const on = b.chargeLimit === l.value}
        <button type="button" class="panel pick" class:selected={on} aria-pressed={on} disabled={busy && !on}
          style="min-height: 186px; gap: 8px"
          onclick={() => !on && run(() => api.setChargeLimit(l.value), `Charge limit set to ${l.value}%`)}>
          <span class="topline"></span>
          <div class="row" style="align-self: stretch">
            <span class="num" style="font-size: 32px" class:accent={on}>{l.value}%</span>
            {#if on}<span class="tag">ACTIVE</span>{/if}
          </div>
          <div class="label" style="font-size: 21px">{l.label}</div>
          <div class="small muted">{l.desc}</div>
        </button>
      {/each}
    </div>
  {:else}
    <div class="panel small muted">This laptop's battery driver doesn't support a charge limit.</div>
  {/if}
</section>

{#if status.extras.superBattery !== null}
  <section class="panel row" class:lit={status.extras.superBattery === true} aria-label="Super Battery">
    <div>
      <div class="label" style="font-size: 20px">Super Battery</div>
      <div class="small muted">MSI's extra power saving for the longest time away from the charger. Performance drops while it's on.</div>
    </div>
    <Switch checked={status.extras.superBattery === true} label="Super Battery" disabled={busy}
      onchange={(on) => run(() => api.setSuperBattery(on), on ? 'Super Battery on' : 'Super Battery off')} />
  </section>
{/if}
