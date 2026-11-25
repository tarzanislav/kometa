<script lang="ts">
  // Malé menu pod tlačidlom „⋯“ – zatvorí sa klikom mimo, klávesom Esc alebo po výbere.
  import type { MenoIkony } from '../lib/ikony'
  import Ikona from './Ikona.svelte'

  export interface PolozkaMenu {
    text: string
    ikona?: MenoIkony
    nebezpecne?: boolean
    akcia: () => void
  }

  let {
    popis,
    polozky,
    text = '',
    ikona = 'viac',
    trieda = 'tl tiche',
  }: { popis: string; polozky: PolozkaMenu[]; text?: string; ikona?: MenoIkony; trieda?: string } = $props()

  let otvorene = $state(false)
  let obal: HTMLDivElement
  let zoznam: HTMLDivElement | undefined = $state()

  $effect(() => {
    if (!otvorene) return
    zoznam?.querySelector('button')?.focus()
    const klik = (e: PointerEvent) => !obal.contains(e.target as Node) && (otvorene = false)
    document.addEventListener('pointerdown', klik)
    return () => document.removeEventListener('pointerdown', klik)
  })

  function klaves(e: KeyboardEvent) {
    if (!otvorene) return
    const tl = [...(zoznam?.querySelectorAll('button') ?? [])]
    const i = tl.indexOf(document.activeElement as HTMLButtonElement)
    if (e.key === 'Escape') {
      e.stopPropagation()
      otvorene = false
      ;(obal.querySelector('button') as HTMLButtonElement)?.focus()
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      tl[(i + (e.key === 'ArrowDown' ? 1 : tl.length - 1)) % tl.length]?.focus()
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="menu" bind:this={obal} onkeydown={klaves}>
  <button
    class={trieda}
    class:ikona={!text}
    aria-haspopup="menu"
    aria-expanded={otvorene}
    aria-label={popis}
    title={popis}
    onclick={() => (otvorene = !otvorene)}
  >
    <Ikona meno={ikona} velkost={text ? 16 : 18} />{text}
  </button>
  {#if otvorene}
    <div class="zoznam" role="menu" bind:this={zoznam}>
      {#each polozky as p (p.text)}
        <button
          role="menuitem"
          class:nebezpecne={p.nebezpecne}
          onclick={() => {
            otvorene = false
            p.akcia()
          }}
        >
          {#if p.ikona}<Ikona meno={p.ikona} velkost={16} />{/if}
          {p.text}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .menu {
    position: relative;
  }
  .zoznam {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 20;
    min-width: 240px;
    padding: var(--s1);
    border: 1px solid var(--line);
    border-radius: var(--r-ctl);
    background: var(--raised);
    box-shadow: var(--shadow-pop);
  }
  .zoznam button {
    display: flex;
    align-items: center;
    gap: var(--s2);
    width: 100%;
    padding: var(--s2) var(--s3);
    border: 0;
    border-radius: calc(var(--r-ctl) - 2px);
    background: none;
    color: var(--ink);
    font: 500 var(--t-s) / 1.3 var(--font-text);
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .zoznam button:hover,
  .zoznam button:focus-visible {
    background: var(--sunken);
    outline: none;
  }
  .zoznam button.nebezpecne {
    color: var(--err);
  }
  .zoznam button.nebezpecne:hover,
  .zoznam button.nebezpecne:focus-visible {
    background: var(--err-soft);
  }
</style>
