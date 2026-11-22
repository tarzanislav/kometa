<script lang="ts">
  import { onMount } from 'svelte'
  import { pripojit } from './lib/api'
  import { app, noc, spustit } from './lib/app.svelte'
  import Bocny from './components/Bocny.svelte'
  import Ikona from './components/Ikona.svelte'
  import Logo from './components/Logo.svelte'
  import Toasty from './components/Toasty.svelte'
  import Playlist from './dialogy/Playlist.svelte'
  import Skener from './dialogy/Skener.svelte'
  import Jazyky from './stranky/Jazyky.svelte'
  import Nastavenia from './stranky/Nastavenia.svelte'
  import Stahovanie from './stranky/Stahovanie.svelte'
  import Ucty from './stranky/Ucty.svelte'

  let chyba = $state('')
  let presuva = $state(false)

  onMount(async () => {
    try {
      await pripojit()
      await spustit()
    } catch (e) {
      chyba = e instanceof Error ? e.message : String(e)
    }
  })

  $effect(() => {
    const html = document.documentElement
    html.dataset.tema = app.nastavenia?.tema ?? 'kometa'
    html.dataset.rezim = noc() ? 'noc' : 'den'
  })

  const ODKAZ = /https?:\/\/\S+/

  function vlozOdkazy(text: string) {
    const odkazy = text.match(/https?:\/\/\S+/g)
    if (!odkazy) return false
    app.prikaz = [app.prikaz.trim(), ...odkazy].filter(Boolean).join('\n')
    app.stranka = 'stahovanie'
    return true
  }

  function prilepit(e: ClipboardEvent) {
    const ciel = e.target as HTMLElement
    if (ciel.closest('input, textarea, [contenteditable]') || app.dialog) return
    const text = e.clipboardData?.getData('text') ?? ''
    if (ODKAZ.test(text) && vlozOdkazy(text)) e.preventDefault()
  }

  function klavesy(e: KeyboardEvent) {
    if (!(e.ctrlKey || e.metaKey) || e.altKey) return
    const stranky = ['stahovanie', 'jazyky', 'ucty', 'nastavenia'] as const
    const i = Number(e.key) - 1
    if (i >= 0 && i < stranky.length) {
      e.preventDefault()
      app.stranka = stranky[i]
    }
  }

  function nadDrop(e: DragEvent) {
    const typy = e.dataTransfer?.types ?? []
    if (typy.includes('text/uri-list') || typy.includes('text/plain')) {
      e.preventDefault()
      presuva = true
    }
  }

  function pustit(e: DragEvent) {
    e.preventDefault()
    presuva = false
    vlozOdkazy(e.dataTransfer?.getData('text/uri-list') || e.dataTransfer?.getData('text/plain') || '')
  }
</script>

<svelte:window onpaste={prilepit} onkeydown={klavesy} />

{#if app.pripravene}
  <div
    class="ram"
    role="application"
    ondragover={nadDrop}
    ondragleave={(e) => e.relatedTarget === null && (presuva = false)}
    ondrop={pustit}
  >
    <Bocny />
    <main>
      {#if !app.spojenie}
        <div class="spojenie" role="alert">
          <Ikona meno="pozor" velkost={18} /> Kometa neodpovedá. Ak si zatvoril jej okno, spusti ju znova.
        </div>
      {/if}
      {#if app.stranka === 'stahovanie'}
        <Stahovanie />
      {:else if app.stranka === 'jazyky'}
        <Jazyky />
      {:else if app.stranka === 'ucty'}
        <Ucty />
      {:else}
        <Nastavenia />
      {/if}
    </main>
    {#if presuva}
      <div class="pustit" aria-hidden="true">
        <div><Ikona meno="odkaz" velkost={28} /><p>Pusti odkaz – pridá sa do sťahovania</p></div>
      </div>
    {/if}
  </div>
  {#if app.dialog?.druh === 'playlist'}
    <Playlist url={app.dialog.url} />
  {:else if app.dialog?.druh === 'skener'}
    <Skener url={app.dialog.url} />
  {/if}
  <Toasty />
{:else}
  <div class="start" aria-busy={!chyba}>
    <Logo velkost={56} animovat />
    {#if chyba}
      <p>Kometa sa nepripojila k svojmu jadru.</p>
      <p class="male tlmene vyber-text">{chyba}</p>
    {/if}
  </div>
{/if}

<style>
  .ram {
    display: flex;
    height: 100%;
  }
  main {
    flex: 1;
    min-width: 0;
    height: 100%;
    overflow: auto;
    scrollbar-gutter: stable;
  }
  .spojenie {
    display: flex;
    gap: var(--s2);
    align-items: center;
    margin: var(--s4) var(--s8) 0;
    padding: var(--s3) var(--s4);
    border-radius: var(--r-ctl);
    background: var(--err-soft);
    color: var(--err);
    font-weight: 600;
  }
  .pustit {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    background: var(--scrim);
    backdrop-filter: blur(2px);
    pointer-events: none;
  }
  .pustit div {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--s3);
    padding: var(--s8) var(--s10);
    border: 2px dashed var(--accent);
    border-radius: 20px;
    background: var(--panel);
    color: var(--accent-ink);
    font-weight: 700;
  }
  .start {
    display: grid;
    place-content: center;
    justify-items: center;
    gap: var(--s3);
    height: 100%;
    text-align: center;
  }
</style>
