// Fonts are bundled with the app (no network needed at runtime).
import '@fontsource-variable/inter';
import '@fontsource/rajdhani/500.css';
import '@fontsource/rajdhani/600.css';
import '@fontsource/rajdhani/700.css';
import '@fontsource/orbitron/500.css';
import '@fontsource/orbitron/600.css';
import './styles.css';
import App from './App.svelte';
import { mount } from 'svelte';

const app = mount(App, { target: document.getElementById('app')! });
export default app;
