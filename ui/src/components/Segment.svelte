<script lang="ts" generics="T extends string">
  import Ikona from './Ikona.svelte'
  import type { MenoIkony } from '../lib/ikony'

  let {
    moznosti,
    hodnota = $bindable(),
    zmena,
    popis,
  }: {
    moznosti: { id: T; text: string; ikona?: MenoIkony; pocet?: number }[]
    hodnota: T
    zmena?: (v: T) => void
    popis: string
  } = $props()

  function vyber(id: T) {
    hodnota = id
    zmena?.(id)
  }

  function klaves(e: KeyboardEvent) {
    const i = moznosti.findIndex((m) => m.id === hodnota)
    const d = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0
    if (!d) return
    e.preventDefault()
    const n = moznosti[(i + d + moznosti.length) % moznosti.length]
    vyber(n.id)
    ;(e.currentTarget as HTMLElement).querySelector<HTMLElement>(`[data-id="${n.id}"]`)?.focus()
  }
</script>

<div class="segment" role="radiogroup" aria-label={popis} tabindex="-1" onkeydown={klaves}>
  {#each moznosti as m (m.id)}
    <button
      type="button"
      role="radio"
      data-id={m.id}
      aria-checked={hodnota === m.id}
      tabindex={hodnota === m.id ? 0 : -1}
      onclick={() => vyber(m.id)}
    >
      {#if m.ikona}<Ikona meno={m.ikona} velkost={16} />{/if}
      <span>{m.text}</span>
      {#if m.pocet != null}<span class="pocet cisla">{m.pocet}</span>{/if}
    </button>
  {/each}
</div>

<style>
  .segment {
    display: inline-flex;
    gap: 2px;
    padding: 3px;
    border-radius: calc(var(--r-ctl) + 2px);
    background: var(--sunken);
  }
  button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 var(--s3);
    border: 0;
    border-radius: var(--r-ctl);
    background: transparent;
    color: var(--muted);
    font: 600 var(--t-s) / 1 var(--font-text);
    white-space: nowrap;
    cursor: pointer;
    transition: background-color 0.15s, color 0.15s;
  }
  button:hover {
    color: var(--ink);
  }
  button[aria-checked='true'] {
    background: var(--raised);
    color: var(--ink);
    box-shadow: var(--shadow-soft);
  }
  .pocet {
    min-width: 18px;
    padding: 1px 5px;
    border-radius: 999px;
    background: var(--line);
    font-size: 11px;
    color: var(--ink-2);
  }
  button[aria-checked='true'] .pocet {
    background: var(--accent-soft);
    color: var(--accent-ink);
  }
</style>
