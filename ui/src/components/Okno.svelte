<script lang="ts">
  import type { Snippet } from 'svelte'
  import Ikona from './Ikona.svelte'

  let {
    nazov,
    popis,
    sirka = 720,
    zavriet,
    children,
    pata,
  }: { nazov: string; popis?: string; sirka?: number; zavriet: () => void; children: Snippet; pata?: Snippet } = $props()

  let dlg: HTMLDialogElement

  $effect(() => {
    dlg.showModal()
    return () => dlg.close()
  })
</script>

<dialog
  bind:this={dlg}
  style:--sirka="{sirka}px"
  oncancel={(e) => {
    e.preventDefault()
    zavriet()
  }}
  onclick={(e) => e.target === dlg && zavriet()}
  aria-labelledby="okno-nazov"
>
  <div class="obal">
    <header>
      <div>
        <h2 id="okno-nazov">{nazov}</h2>
        {#if popis}<p class="tlmene male">{popis}</p>{/if}
      </div>
      <button class="tl ikona" onclick={zavriet} aria-label="Zavrieť"><Ikona meno="zavriet" /></button>
    </header>
    <div class="telo">{@render children()}</div>
    {#if pata}<footer>{@render pata()}</footer>{/if}
  </div>
</dialog>

<style>
  dialog {
    width: min(var(--sirka), calc(100vw - 48px));
    max-height: calc(100vh - 64px);
    padding: 0;
    border: 1px solid var(--line);
    border-radius: calc(var(--r-panel) + 4px);
    background: var(--panel);
    color: var(--ink);
    box-shadow: var(--shadow-pop);
  }
  dialog[open] {
    animation: otvor 0.22s var(--ease);
  }
  dialog::backdrop {
    background: var(--scrim);
    backdrop-filter: blur(3px);
  }
  .obal {
    display: flex;
    flex-direction: column;
    max-height: calc(100vh - 66px);
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--s4);
    padding: var(--s5) var(--s5) var(--s3) var(--s6);
  }
  header p {
    margin-top: 2px;
  }
  .telo {
    padding: 0 var(--s6) var(--s4);
    overflow: auto;
    min-height: 0;
  }
  footer {
    display: flex;
    align-items: center;
    gap: var(--s3);
    padding: var(--s4) var(--s6);
    border-top: 1px solid var(--line);
    background: var(--bg);
    border-radius: 0 0 calc(var(--r-panel) + 4px) calc(var(--r-panel) + 4px);
  }
  @keyframes otvor {
    from {
      opacity: 0;
      translate: 0 8px;
      scale: 0.985;
    }
  }
</style>
