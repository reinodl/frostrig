export type ThemeId = 'crimson' | 'neon' | 'synth' | 'toxic' | 'arctic';

export interface Theme {
  id: ThemeId;
  label: string;
  a1: string;   // main accent
  a2: string;   // second accent, used in gradients
  bg: string;
  panel: string;
  border: string;
  text: string;
  muted: string;
  on: string;   // text on top of an accent fill
}

export const THEMES: Theme[] = [
  { id: 'crimson', label: 'Crimson', a1: '#FF2E4D', a2: '#FF8A3D', bg: '#09060A', panel: '#160C11', border: '#3A1D27', text: '#F7EEF0', muted: '#AE9AA1', on: '#14060A' },
  { id: 'neon', label: 'Neon City', a1: '#00E5FF', a2: '#FF2BD6', bg: '#050813', panel: '#0B1222', border: '#1C2B4C', text: '#EAF6FF', muted: '#93A7C8', on: '#021018' },
  { id: 'synth', label: 'Synthwave', a1: '#FF4FD8', a2: '#7C5CFF', bg: '#0A0514', panel: '#160B26', border: '#33204F', text: '#F7EEFF', muted: '#AC9BC8', on: '#16051A' },
  { id: 'toxic', label: 'Toxic', a1: '#39FF88', a2: '#D4FF3A', bg: '#040A07', panel: '#0A1711', border: '#173A28', text: '#EDFFF4', muted: '#92B8A4', on: '#03140A' },
  { id: 'arctic', label: 'Arctic', a1: '#5CC8FF', a2: '#B18CFF', bg: '#040911', panel: '#0B1420', border: '#1C2D46', text: '#EEF6FF', muted: '#97AAC3', on: '#04101C' }
];

export function themeById(id: string | null): Theme {
  return THEMES.find((t) => t.id === id) ?? THEMES[0];
}

export function applyTheme(t: Theme): void {
  const s = document.documentElement.style;
  s.setProperty('--a1', t.a1);
  s.setProperty('--a2', t.a2);
  s.setProperty('--bg', t.bg);
  s.setProperty('--panel', t.panel);
  s.setProperty('--border', t.border);
  s.setProperty('--text', t.text);
  s.setProperty('--muted', t.muted);
  s.setProperty('--on', t.on);
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', t.bg);
}

// App-only preferences live in the webview's storage; hardware settings live in the service.
const PREFIX = 'frostrig:';
export function loadPref(key: string, fallback: string): string {
  try { return localStorage.getItem(PREFIX + key) ?? fallback; } catch { return fallback; }
}
export function savePref(key: string, value: string): void {
  try { localStorage.setItem(PREFIX + key, value); } catch { /* storage unavailable: keep in memory */ }
}
