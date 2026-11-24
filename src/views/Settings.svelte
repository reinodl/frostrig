<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import Switch from '../components/Switch.svelte';
  import { api, errorText, type Status } from '../lib/api';
  import { THEMES, type ThemeId } from '../lib/themes';

  let { status, themeId, autostart, closeToTray, scanlines, onTheme, onAutostart, onCloseToTray, onScanlines }: {
    status: Status | null;
    themeId: ThemeId;
    autostart: boolean;
    closeToTray: boolean;
    scanlines: boolean;
    onTheme: (id: ThemeId) => void;
    onAutostart: (on: boolean) => void;
    onCloseToTray: (on: boolean) => void;
    onScanlines: (on: boolean) => void;
  } = $props();

  let reportNote = $state('');
  async function copyReport() {
    try {
      await navigator.clipboard.writeText(await api.report());
      reportNote = 'Copied. Paste it into your bug report.';
    } catch (e) {
      reportNote = `Couldn't copy the report: ${errorText(e)}`;
    }
  }

  let rows = $derived([
    { label: 'Start with the desktop', desc: 'Opens in the system tray when you log in.', on: autostart, set: onAutostart },
    { label: 'Close to tray', desc: 'Closing the window keeps FrostRig in the tray.', on: closeToTray, set: onCloseToTray },
    { label: 'Scanline effect', desc: 'Faint CRT-style lines over the app.', on: scanlines, set: onScanlines }
  ]);
</script>

<section class="section" aria-labelledby="theme-h">
  <h2 id="theme-h" class="section-label">Theme</h2>
  <div class="grid-5">
    {#each THEMES as t (t.id)}
      {@const on = t.id === themeId}
      <button type="button" class="panel pick" class:selected={on} aria-pressed={on} style="padding: 14px; gap: 12px"
        onclick={() => onTheme(t.id)}>
        <span class="topline"></span>
        <div class="preview" aria-hidden="true"
          style="border-color: {t.border}; background-color: {t.bg}; background-image: linear-gradient(90deg, transparent 18%, {t.a1} 18%, {t.a1} 21%, transparent 21%), radial-gradient(circle at 80% 30%, {t.a1}8C, transparent 45%), radial-gradient(circle at 30% 100%, {t.a2}80, transparent 50%); box-shadow: inset 0 0 18px {t.a1}40">
        </div>
        <div class="row" style="align-self: stretch">
          <span class="label" style="font-size: 17px">{t.label}</span>
          {#if on}<Icon name="check" size={16} />{/if}
        </div>
      </button>
    {/each}
  </div>
</section>

<section class="panel" style="padding: 6px 22px; max-width: 760px">
  {#each rows as r, i (r.label)}
    <div class="row setting" class:first={i === 0}>
      <div>
        <div style="font-weight: 600; font-size: 15px">{r.label}</div>
        <div class="small muted">{r.desc}</div>
      </div>
      <Switch checked={r.on} label={r.label} onchange={r.set} />
    </div>
  {/each}
</section>

<div class="row" style="justify-content: flex-start; gap: 14px">
  <button type="button" class="btn" onclick={copyReport}>Copy system report</button>
  <span class="small muted">{reportNote || 'What FrostRig found on this computer, for bug reports. No serial numbers or personal data.'}</span>
</div>

<p class="small muted" style="margin: 0">
  FrostRig {status?.version ?? ''}
  {#if status?.device.model} · {status.device.model}{/if}
  {#if status?.device.ecFirmware} · EC firmware {status.device.ecFirmware}{/if}
  · Your settings are kept by the background service and come back after restarts and sleep.
</p>
<p class="small muted" style="margin: 0">
  FrostRig is an independent project, not affiliated with or endorsed by Micro-Star International.
  MSI is a trademark of Micro-Star International Co., Ltd.
</p>

<style>
  .preview { align-self: stretch; height: 64px; border-radius: 3px; border: 1px solid; }
  .setting { padding: 16px 0; border-top: 1px solid var(--border); }
  .setting.first { border-top: none; }
</style>
