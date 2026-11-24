<script lang="ts">
  // Small inline stroke icons, so the app needs no icon package.
  type Name = 'home' | 'battery' | 'sliders' | 'settings' | 'keyboard' | 'leaf' | 'scale' | 'gauge' | 'flame' | 'fan' | 'bolt' | 'check' | 'alert';
  let { name, size = 20 }: { name: Name; size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
  {#if name === 'home'}
    <path d="M3 10.5 12 3l9 7.5" /><path d="M5 9.5V21h14V9.5" />
  {:else if name === 'battery'}
    <rect x="2" y="7" width="17" height="10" rx="2" /><path d="M22 11v2" /><path d="M6 10v4M9.5 10v4" />
  {:else if name === 'sliders'}
    <path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12" /><circle cx="16" cy="6" r="2" /><circle cx="10" cy="12" r="2" /><circle cx="18" cy="18" r="2" />
  {:else if name === 'settings'}
    <circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
  {:else if name === 'keyboard'}
    <rect x="2" y="5" width="20" height="14" rx="2" /><path d="M6 9h.01M10 9h.01M14 9h.01M18 9h.01M6 13h.01M10 13h.01M14 13h.01M18 13h.01M8 16h8" />
  {:else if name === 'leaf'}
    <path d="M11 20A7 7 0 0 1 9.8 6.1C15.5 5 17 4.5 19 2c1 2 2 4.2 2 8 0 5.5-4.8 10-10 10Z" /><path d="M2 21c0-3 1.9-5.4 5.1-6" />
  {:else if name === 'scale'}
    <path d="M12 3v18M7 21h10M5 7h14" /><path d="m5 7-3 7a3.5 3.5 0 0 0 6 0Z" /><path d="m19 7-3 7a3.5 3.5 0 0 0 6 0Z" />
  {:else if name === 'gauge'}
    <path d="M12 14l4-4" /><path d="M3.3 19a9.5 9.5 0 1 1 17.4 0" />
  {:else if name === 'flame'}
    <path d="M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.4-.5-2-1-3-1.1-2.1-.2-4 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.2.4-2.3 1-3.3.4 1.5 1.3 2.8 2.5 2.8Z" />
  {:else if name === 'fan'}
    <path d="M12 12c-3-5 0-9 3-9 2 0 3 2 2 4l-5 5Z" /><path d="M12 12c5-3 9 0 9 3 0 2-2 3-4 2l-5-5Z" /><path d="M12 12c3 5 0 9-3 9-2 0-3-2-2-4l5-5Z" /><path d="M12 12c-5 3-9 0-9-3 0-2 2-3 4-2l5 5Z" />
  {:else if name === 'bolt'}
    <path d="M13 2 4 14h7l-1 8 9-12h-7z" />
  {:else if name === 'check'}
    <path d="M20 6 9 17l-5-5" />
  {:else if name === 'alert'}
    <path d="M12 9v4M12 17h.01" /><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z" />
  {/if}
</svg>
