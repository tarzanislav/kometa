<script lang="ts">
  import type { Snippet } from 'svelte'

  let {
    zapnute = $bindable(false),
    nazov,
    popis,
    zmena,
    zakazane = false,
  }: { zapnute?: boolean; nazov: string; popis?: Snippet | string; zmena?: (v: boolean) => void; zakazane?: boolean } =
    $props()

  function prepni() {
    if (zakazane) return
    zapnute = !zapnute
    zmena?.(zapnute)
  }
</script>

<div class="riadok" class:zakazane>
  <button type="button" role="switch" aria-checked={zapnute} aria-label={nazov} onclick={prepni} disabled={zakazane}>
    <span class="gulicka"></span>
  </button>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="text" onclick={prepni}>
    <span class="nazov">{nazov}</span>
    {#if typeof popis === 'string'}<span class="popis">{popis}</span>{:else if popis}<span class="popis">{@render popis()}</span>{/if}
  </div>
</div>

<style>
  .riadok {
    display: flex;
    gap: var(--s3);
    align-items: flex-start;
  }
  .zakazane {
    opacity: 0.5;
  }
  button {
    flex: none;
    position: relative;
    width: 38px;
    height: 22px;
    margin-top: 1px;
    padding: 0;
    border: 0;
    border-radius: 999px;
    background: var(--line-strong);
    cursor: pointer;
    transition: background-color 0.2s var(--ease);
  }
  button[aria-checked='true'] {
    background: var(--accent);
  }
  .gulicka {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: oklch(100% 0 0);
    box-shadow: 0 1px 2px oklch(0% 0 0 / 0.25);
    transition: translate 0.2s var(--ease);
  }
  button[aria-checked='true'] .gulicka {
    translate: 16px 0;
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    cursor: pointer;
    min-width: 0;
  }
  .nazov {
    font-weight: 600;
    color: var(--ink);
  }
  .popis {
    font-size: var(--t-s);
    color: var(--muted);
    max-width: 62ch;
  }
</style>
