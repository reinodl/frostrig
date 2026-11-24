<script lang="ts">
  let { checked, label, disabled = false, onchange }: {
    checked: boolean;
    label: string;
    disabled?: boolean;
    onchange: (next: boolean) => void;
  } = $props();
</script>

<button
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={label}
  class="switch"
  class:on={checked}
  {disabled}
  onclick={() => onchange(!checked)}
><span class="knob"></span></button>
