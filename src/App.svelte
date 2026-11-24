<script lang="ts">
  import logo from './assets/frostrig.svg';
  import { onMount } from 'svelte';
  import Icon from './components/Icon.svelte';
  import Home from './views/Home.svelte';
  import Battery from './views/Battery.svelte';
  import Advanced from './views/Advanced.svelte';
  import Device from './views/Device.svelte';
  import Settings from './views/Settings.svelte';
  import { api, errorText, getStatus, type Status } from './lib/api';
  import { applyTheme, loadPref, savePref, themeById, type ThemeId } from './lib/themes';
  import { pageActions } from './lib/pageActions.svelte.js';

  type Tab = 'home' | 'battery' | 'advanced' | 'device' | 'settings';
  const NAV: { id: Tab; label: string; icon: 'home' | 'battery' | 'sliders' | 'keyboard' }[] = [
    { id: 'home', label: 'Home', icon: 'home' },
    { id: 'battery', label: 'Battery', icon: 'battery' },
    { id: 'advanced', label: 'Advanced', icon: 'sliders' },
    { id: 'device', label: 'Device', icon: 'keyboard' }
  ];
  const TITLES: Record<Tab, string> = { home: 'Home', battery: 'Battery', advanced: 'Advanced', device: 'Device', settings: 'Settings' };

  // Tabs only appear when the machine has something to show on them.
  function hasTab(id: Tab): boolean {
    if (!status) return id !== 'device';
    if (id === 'battery') return status.battery.present;
    if (id === 'device') {
      const x = status.extras;
      return x.webcam !== null || x.webcamBlock !== null || x.fnKey !== null || x.kbdBacklight !== null;
    }
    return true;
  }

  let tab = $state<Tab>('home');
  let status = $state<Status | null>(null);
  let offline = $state<string | null>(null);
  let busy = $state(false);
  let toast = $state<{ text: string; error: boolean } | null>(null);

  let themeId = $state<ThemeId>(themeById(loadPref('theme', 'crimson')).id);
  let scanlines = $state(loadPref('scanlines', '1') === '1');
  let closeToTray = $state(loadPref('closeToTray', '1') === '1');
  let autostart = $state(false);

  $effect(() => applyTheme(themeById(themeId)));

  async function refresh() {
    try {
      status = await getStatus();
      offline = null;
    } catch (e) {
      offline = errorText(e);
    }
  }

  onMount(() => {
    refresh();
    api.setCloseToTray(closeToTray).catch(() => {});
    api.getAutostart().then((on) => (autostart = on)).catch(() => {});
    // Poll while visible; the window is hidden when it sits in the tray.
    const timer = setInterval(() => { if (!document.hidden) refresh(); }, 2000);
    return () => clearInterval(timer);
  });

  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  function notify(text: string, error = false) {
    clearTimeout(toastTimer);
    toast = { text, error };
    toastTimer = setTimeout(() => (toast = null), error ? 7000 : 2200);
  }

  // Every hardware change goes through here: one click, no confirmation.
  // The service validates, applies, reads back and rolls back on failure.
  async function run(action: () => Promise<unknown>, success: string): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await action();
      notify(success);
    } catch (e) {
      notify(errorText(e), true);
    } finally {
      busy = false;
      await refresh();
    }
  }

  function go(next: Tab) {
    if (next === tab) return;
    if (pageActions.pending) notify('Unapplied changes were discarded');
    tab = next;
  }

  function setTheme(id: ThemeId) {
    themeId = id;
    savePref('theme', id);
  }
  function setScanlines(on: boolean) {
    scanlines = on;
    savePref('scanlines', on ? '1' : '0');
  }
  function setCloseToTray(on: boolean) {
    closeToTray = on;
    savePref('closeToTray', on ? '1' : '0');
    api.setCloseToTray(on).catch(() => {});
  }
  async function setAutostart(on: boolean) {
    try {
      await api.setAutostart(on);
      autostart = on;
    } catch (e) {
      notify(errorText(e), true);
    }
  }
</script>

<div class="app">
  {#if scanlines}<div class="scanlines" aria-hidden="true"></div>{/if}

  <nav class="nav" aria-label="Main">
    <div class="brand">
      <img class="brand-mark" src={logo} alt="" width="42" height="42" />
      <div>
        <div class="brand-name">FROSTRIG</div>
        <div class="brand-sub">FOR LINUX</div>
      </div>
    </div>
    {#each NAV.filter((n) => hasTab(n.id)) as n (n.id)}
      <button type="button" class="nav-btn" class:active={tab === n.id} aria-current={tab === n.id ? 'page' : undefined}
        onclick={() => go(n.id)}>
        <Icon name={n.icon} /> <span>{n.label}</span>
      </button>
    {/each}
    <div class="nav-spacer"></div>
    <button type="button" class="nav-btn" class:active={tab === 'settings'} aria-current={tab === 'settings' ? 'page' : undefined}
      onclick={() => go('settings')}>
      <Icon name="settings" /> <span>Settings</span>
    </button>
  </nav>

  <main class="main">
    <header class="header">
      <div>
        <div class="eyebrow">{status?.device.model ?? 'Your computer'}</div>
        <h1 class="title">{TITLES[tab]}</h1>
        <div class="underline" aria-hidden="true"></div>
      </div>
      <div class="header-right">
        {#if pageActions.pending}
          <span class="label accent pending-note">Changes not applied</span>
          <button type="button" class="btn" disabled={busy} onclick={() => pageActions.undo?.()}>Undo</button>
          <button type="button" class="btn solid apply" disabled={busy} onclick={() => pageActions.apply?.()}>Apply</button>
        {:else if pageActions.reset}
          <button type="button" class="btn" disabled={busy} onclick={() => pageActions.reset?.()}>Reset to defaults</button>
        {/if}
        <div class="chip">
          <span class="dot" class:off={offline !== null}></span>
          <span>{offline !== null ? 'Offline' : 'Online'}</span>
        </div>
      </div>
    </header>

    {#if tab === 'settings'}
      <Settings {status} {themeId} {autostart} {closeToTray} {scanlines}
        onTheme={setTheme} onAutostart={setAutostart} onCloseToTray={setCloseToTray} onScanlines={setScanlines} />
    {:else if offline !== null && status === null}
      <section class="panel notice">
        <div class="row" style="justify-content: flex-start; gap: 10px; color: var(--hot)">
          <Icon name="alert" /> <span class="label" style="font-size: 20px">Can't reach the FrostRig service</span>
        </div>
        <p class="small muted">{offline}</p>
        <p class="small muted" style="margin-bottom: 0">
          If you just installed FrostRig, start the service once with <code>sudo systemctl enable --now frostrigd</code>.
        </p>
      </section>
    {:else if status === null}
      <p class="muted">Connecting…</p>
    {:else if tab === 'home'}
      <Home {status} {busy} {run} goBattery={() => go('battery')} />
    {:else if tab === 'battery'}
      <Battery {status} {busy} {run} />
    {:else if tab === 'advanced'}
      <Advanced {status} {busy} {run} />
    {:else if tab === 'device'}
      <Device {status} {busy} {run} />
    {/if}
  </main>

  {#if toast}
    <div class="toast" class:error={toast.error} role="status">
      <span style="color: {toast.error ? 'var(--hot)' : 'var(--a1)'}; display: inline-flex">
        <Icon name={toast.error ? 'alert' : 'check'} size={18} />
      </span>
      <span>{toast.text}</span>
    </div>
  {/if}
</div>
