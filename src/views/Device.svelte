<script lang="ts">
  import Switch from '../components/Switch.svelte';
  import { api, type Status } from '../lib/api';

  let { status, busy, run }: {
    status: Status;
    busy: boolean;
    run: (action: () => Promise<unknown>, success: string) => Promise<void>;
  } = $props();

  const SIDES = ['left', 'right'] as const;
  let x = $derived(status.extras);
  let blocked = $derived(x.webcamBlock === true);

  // msi-ec's backlight has four steps; other drivers may have two or three.
  const NAMES: Record<number, string[]> = {
    1: ['Off', 'On'],
    2: ['Off', 'Low', 'High'],
    3: ['Off', 'Low', 'Medium', 'High']
  };
  let levels = $derived(
    x.kbdBacklight
      ? (NAMES[x.kbdBacklight.max] ?? Array.from({ length: x.kbdBacklight.max + 1 }, (_, i) => (i === 0 ? 'Off' : String(i))))
      : []
  );
</script>

<div class="grid-2">
  {#if x.webcam !== null || x.webcamBlock !== null}
    <section class="panel" style="display: flex; flex-direction: column" aria-labelledby="cam-h">
      <h2 id="cam-h" class="label" style="font-size: 21px; margin-bottom: 6px">Webcam</h2>
      {#if x.webcam !== null}
        <div class="row setting first">
          <div>
            <div style="font-weight: 600; font-size: 15px">Webcam</div>
            <div class="small muted">{blocked ? 'Blocked: turn off the block to use it' : 'The same as the webcam key on the keyboard'}</div>
          </div>
          <Switch checked={x.webcam === true} label="Webcam" disabled={busy || blocked}
            onchange={(on) => run(() => api.setWebcam(on), on ? 'Webcam on' : 'Webcam off')} />
        </div>
      {/if}
      {#if x.webcamBlock !== null}
        <div class="row setting" class:first={x.webcam === null}>
          <div>
            <div style="font-weight: 600; font-size: 15px">Block webcam</div>
            <div class="small muted">Keeps it off, even if the webcam key is pressed</div>
          </div>
          <Switch checked={blocked} label="Block webcam" disabled={busy}
            onchange={(on) => run(() => api.setWebcamBlock(on), on ? 'Webcam blocked' : 'Webcam unblocked')} />
        </div>
      {/if}
    </section>
  {/if}

  {#if x.fnKey !== null || x.kbdBacklight}
    <section class="panel" style="display: flex; flex-direction: column" aria-labelledby="kb-h">
      <h2 id="kb-h" class="label" style="font-size: 21px; margin-bottom: 6px">Keyboard</h2>
      {#if x.fnKey !== null}
        <div class="row setting first">
          <div>
            <div style="font-weight: 600; font-size: 15px">Fn key position</div>
            <div class="small muted">Swaps the Fn and Windows keys</div>
          </div>
          <div class="seg" role="group" aria-label="Fn key position">
            {#each SIDES as side (side)}
              <button type="button" class:active={x.fnKey === side} aria-pressed={x.fnKey === side} disabled={busy}
                onclick={() => x.fnKey !== side && run(() => api.setFnKey(side), `Fn key on the ${side}`)}>{side === 'left' ? 'Left' : 'Right'}</button>
            {/each}
          </div>
        </div>
      {/if}
      {#if x.kbdBacklight}
        <div class="row setting" class:first={x.fnKey === null}>
          <div>
            <div style="font-weight: 600; font-size: 15px">Backlight</div>
            <div class="small muted">Keyboard light brightness</div>
          </div>
          <div class="seg" role="group" aria-label="Keyboard backlight">
            {#each levels as name, i (i)}
              <button type="button" class:active={x.kbdBacklight.level === i} aria-pressed={x.kbdBacklight.level === i} disabled={busy}
                onclick={() => x.kbdBacklight?.level !== i && run(() => api.setKeyboardBacklight(i), `Keyboard backlight ${name.toLowerCase()}`)}>{name}</button>
            {/each}
          </div>
        </div>
      {/if}
    </section>
  {/if}
</div>

<style>
  .setting { padding: 14px 0; border-top: 1px solid var(--border); }
  .setting.first { border-top: none; }
</style>
