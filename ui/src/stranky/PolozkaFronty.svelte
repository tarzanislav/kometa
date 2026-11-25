<script lang="ts">
  import { app, skus, toast } from '../lib/app.svelte'
  import { dlzka, rychlostText, velkost, zostava } from '../lib/format'
  import { ikonaZdroja } from '../lib/ikony'
  import type { Polozka } from '../lib/typy'
  import Ikona from '../components/Ikona.svelte'
  import Kometa from '../components/Kometa.svelte'
  import Menu from '../components/Menu.svelte'

  let { p }: { p: Polozka } = $props()
  let podrobnosti = $state(false)
  let noveHeslo = $state('')
  const rozb = $derived(p.rozbalenie)

  async function zadatHeslo() {
    if (!noveHeslo) return
    await skus('zadat_heslo', p.id, noveHeslo)
    noveHeslo = ''
  }
  let bezObrazka = $state(false)

  const ULOZISKA = ['Webshare', 'Fastshare', 'Datoid']
  const formatNazov = $derived(
    p.format === 'subor' || ULOZISKA.includes(p.zdroj)
      ? 'Súbor'
      : (app.init?.formaty.find((f) => f.id === p.format)?.nazov ?? ''),
  )
  const podiel = $derived(p.podiel ?? (p.celkom ? p.hotovo / p.celkom : null))
  const kometaStav = $derived(
    p.stav === 'stahuje' ? 'bezi' : p.stav === 'pauza' ? 'pauza' : p.stav === 'chyba' ? 'chyba' : 'caka',
  )
  const velkostText = $derived.by(() => {
    if (p.stav === 'hotovo') return velkost(p.celkom || p.hotovo)
    if (p.podiel != null) return `${p.krok || 'Pripravujem'} (${Math.floor(p.podiel * 100)} %)`
    if (!p.hotovo && !p.celkom) return ''
    if (p.celkom) return `${velkost(p.hotovo)} z ${velkost(p.celkom)} (${Math.floor((podiel ?? 0) * 100)} %)`
    return velkost(p.hotovo)
  })

  async function kopirovat() {
    try {
      await navigator.clipboard.writeText(p.url)
      toast('Odkaz je skopírovaný.', 'ok')
    } catch {
      toast('Odkaz sa nepodarilo skopírovať.', 'chyba')
    }
  }
</script>

<article class="polozka {p.stav}" aria-label={p.nazov || p.url}>
  <div class="nahlad" class:hotovo={p.stav === 'hotovo'}>
    {#if p.nahlad && !bezObrazka}
      <img src={p.nahlad} alt="" loading="lazy" referrerpolicy="no-referrer" onerror={() => (bezObrazka = true)} />
    {:else}
      <Ikona meno={ikonaZdroja(p.zdroj)} velkost={p.stav === 'hotovo' ? 18 : 22} />
    {/if}
    {#if p.dlzka && p.stav !== 'hotovo'}<span class="dlzka cisla">{dlzka(p.dlzka)}</span>{/if}
  </div>

  <div class="hlavne">
    <h3 title={p.nazov || p.url}>{p.nazov || p.url}</h3>
    <div class="meta">
      <span class="stitok">{p.zdroj}</span>
      {#if p.stav === 'hotovo' && p.subor && p.subor !== p.nazov}
        <span class="subor vyber-text" title={p.subor}>{p.subor}</span>
      {:else if p.jazyky && p.stav !== 'hotovo'}
        <span class="jazyky-stitok" title="Jazykové stopy">{p.jazyky}</span>
      {:else if formatNazov && p.stav !== 'hotovo'}
        <span>{formatNazov}</span>
      {/if}
      {#if velkostText}<span class="cisla">{velkostText}</span>{/if}
      {#if p.autor && p.stav !== 'hotovo'}<span class="autor">{p.autor}</span>{/if}
    </div>
    {#if p.stav === 'stahuje' || p.stav === 'pauza' || p.stav === 'caka'}
      <div class="priebeh"><Kometa hodnota={podiel} stav={kometaStav} popis="Priebeh sťahovania" /></div>
    {:else if p.stav === 'chyba'}
      <p class="chyba vyber-text">{p.chyba}</p>
      {#if p.treba_heslo}
        <form
          class="heslo"
          onsubmit={(e) => {
            e.preventDefault()
            zadatHeslo()
          }}
        >
          <input class="pole" type="password" bind:value={noveHeslo} placeholder="Heslo k súboru" aria-label="Heslo k súboru" />
          <button class="tl hlavne" disabled={!noveHeslo}>Pokračovať</button>
        </form>
      {/if}
      {#if p.podrobnosti}
        <button class="viac" onclick={() => (podrobnosti = !podrobnosti)} aria-expanded={podrobnosti}>
          {podrobnosti ? 'Skryť podrobnosti' : 'Podrobnosti'}
        </button>
        {#if podrobnosti}<pre class="vyber-text">{p.podrobnosti}</pre>{/if}
      {/if}
    {:else if p.stav === 'hotovo' && rozb}
      <div class="archiv {rozb.stav}">
        <Ikona meno={rozb.stav === 'heslo' ? 'zamok' : 'balik'} velkost={15} />
        {#if rozb.stav === 'bezi'}
          <span>Rozbaľujem{rozb.podiel != null ? ` (${Math.floor(rozb.podiel * 100)} %)` : '…'}</span>
          <div class="priebeh-archivu"><Kometa hodnota={rozb.podiel ?? null} stav="bezi" popis="Priebeh rozbaľovania" /></div>
        {:else if rozb.stav === 'hotovo'}
          <span>{rozb.text}{rozb.ciel ? ` → ${rozb.ciel}` : ''}</span>
        {:else if rozb.stav === 'vypnute'}
          <span>Archív</span>
          <button class="tl male-tl" onclick={() => skus('rozbalit', p.id)}>Rozbaliť</button>
        {:else if rozb.stav === 'heslo'}
          <span>{rozb.text}</span>
          <form
            class="heslo"
            onsubmit={(e) => {
              e.preventDefault()
              zadatHeslo()
            }}
          >
            <input class="pole" type="password" bind:value={noveHeslo} placeholder="Heslo k archívu" aria-label="Heslo k archívu" />
            <button class="tl hlavne" disabled={!noveHeslo}>Rozbaliť</button>
          </form>
        {:else if rozb.stav === 'chyba'}
          <span class="chyba">{rozb.text}</span>
          <button class="tl male-tl" onclick={() => skus('rozbalit', p.id)}>Skúsiť znova</button>
        {:else}
          <span>{rozb.text}</span>
        {/if}
      </div>
    {:else if p.stav === 'rozbalene'}
      <p class="male tlmene">Na stránke bolo viac videí – každé je vo fronte samostatne.</p>
    {/if}
  </div>

  <div class="vpravo">
    {#if p.stav === 'stahuje'}
      <strong class="cisla">{rychlostText(p.rychlost)}</strong>
      <span class="male tlmene cisla"
        >{p.zostava ? `ešte ${zostava(p.zostava)}` : p.podiel != null ? `${Math.floor(p.podiel * 100)} %` : 'pripravujem…'}</span
      >
    {:else if p.stav === 'pauza'}
      <span class="stav pauza">Pozastavené</span>
    {:else if p.stav === 'caka'}
      <span class="stav">Čaká</span>
    {:else if p.stav === 'hotovo'}
      <span class="stav ok"><Ikona meno="fajka" velkost={16} /> Hotovo</span>
    {:else if p.stav === 'chyba'}
      <span class="stav err">Nepodarilo sa</span>
    {/if}
  </div>

  <div class="akcie">
    {#if p.stav === 'stahuje' || p.stav === 'caka'}
      <button class="tl ikona" title="Pozastaviť" aria-label="Pozastaviť" onclick={() => skus('pozastavit', [p.id])}>
        <Ikona meno="pauza" />
      </button>
    {:else if p.stav === 'pauza'}
      <button class="tl ikona" title="Pokračovať" aria-label="Pokračovať" onclick={() => skus('spustit', [p.id])}>
        <Ikona meno="play" />
      </button>
    {:else if p.stav === 'chyba'}
      <button class="tl ikona" title="Skúsiť znova" aria-label="Skúsiť znova" onclick={() => skus('spustit', [p.id])}>
        <Ikona meno="znova" />
      </button>
    {:else if p.stav === 'hotovo'}
      <button class="tl ikona" title="Otvoriť priečinok" aria-label="Otvoriť priečinok" onclick={() => skus('otvorit', p.id)}>
        <Ikona meno="priecinok" />
      </button>
    {/if}
    <button class="tl ikona" title="Kopírovať odkaz" aria-label="Kopírovať odkaz" onclick={kopirovat}>
      <Ikona meno="kopirovat" />
    </button>
    {#if p.stav === 'hotovo'}
      <Menu
        popis="Odstrániť"
        ikona="kos"
        trieda="tl ikona nebezpecne"
        polozky={[
          { text: 'Odstrániť zo zoznamu (súbor ostane)', ikona: 'zavriet', akcia: () => skus('odstranit', [p.id]) },
          {
            text: 'Odstrániť aj súbor…',
            ikona: 'kos',
            nebezpecne: true,
            akcia: () => (app.dialog = { druh: 'zmazat', co: 'subory', ids: [p.id] }),
          },
        ]}
      />
    {:else}
      <button
        class="tl ikona nebezpecne"
        title="Zrušiť a odstrániť"
        aria-label="Zrušiť a odstrániť"
        onclick={() => skus('odstranit', [p.id])}
      >
        <Ikona meno="kos" />
      </button>
    {/if}
  </div>
</article>

<style>
  .heslo {
    display: flex;
    gap: var(--s2);
    margin-top: var(--s2);
    max-width: 420px;
  }
  .heslo .pole {
    height: 34px;
  }
  .archiv {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s2);
    margin-top: var(--s2);
    color: var(--ink-2);
    font-size: var(--t-s);
  }
  .archiv.hotovo {
    color: var(--ok);
  }
  .archiv .heslo {
    margin-top: 0;
    flex-basis: 100%;
  }
  .priebeh-archivu {
    flex: 1;
    min-width: 160px;
  }
  .male-tl {
    height: 28px;
    padding: 0 var(--s3);
  }
  .polozka {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto auto;
    align-items: center;
    gap: var(--s4);
    padding: var(--s3) var(--s3) var(--s3) var(--s3);
    border-radius: var(--r-panel);
    transition: background-color 0.15s;
  }
  .polozka:hover,
  .polozka:focus-within {
    background: var(--panel);
  }
  .nahlad {
    position: relative;
    display: grid;
    place-items: center;
    width: 128px;
    aspect-ratio: 16 / 9;
    overflow: hidden;
    border-radius: 10px;
    background: linear-gradient(135deg, var(--accent-soft), var(--sunken) 75%);
    color: var(--accent-ink);
  }
  .nahlad.hotovo {
    width: 72px;
    border-radius: 8px;
  }
  .nahlad img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .dlzka {
    position: absolute;
    right: 5px;
    bottom: 5px;
    padding: 1px 5px;
    border-radius: 5px;
    background: oklch(15% 0.02 var(--h) / 0.78);
    color: oklch(98% 0 0);
    font-size: 11px;
    font-weight: 700;
  }
  .hlavne {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  h3 {
    font-family: var(--font-text);
    font-size: var(--t-m);
    font-weight: 650;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hotovo h3 {
    font-weight: 600;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px var(--s3);
    min-width: 0;
    overflow: hidden;
    font-size: var(--t-s);
    color: var(--muted);
  }
  .meta > span {
    white-space: nowrap;
  }
  .subor,
  .autor {
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .priebeh {
    padding: 6px 8px 2px 0;
  }
  .chyba {
    color: var(--err);
    font-size: var(--t-s);
    font-weight: 600;
  }
  .viac {
    align-self: flex-start;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent-ink);
    font: 600 var(--t-xs) / 1.4 var(--font-text);
    cursor: pointer;
  }
  pre {
    margin: 0;
    padding: var(--s2) var(--s3);
    border-radius: var(--r-ctl);
    background: var(--sunken);
    color: var(--ink-2);
    font: 400 var(--t-xs) / 1.5 var(--font-text);
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 160px;
    overflow: auto;
  }
  .vpravo {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 2px;
    min-width: 92px;
    text-align: right;
  }
  .vpravo strong {
    font: 650 var(--t-l) / 1.1 var(--font-display);
    font-variation-settings: 'opsz' 24;
  }
  .stav {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--t-s);
    font-weight: 600;
    color: var(--muted);
  }
  .stav.pauza {
    color: var(--warn);
  }
  .stav.ok {
    color: var(--ok);
  }
  .stav.err {
    color: var(--err);
  }
  .akcie {
    display: flex;
    gap: 2px;
    opacity: 0.55;
    transition: opacity 0.15s;
  }
  .polozka:hover .akcie,
  .polozka:focus-within .akcie {
    opacity: 1;
  }
  @media (max-width: 860px) {
    .polozka {
      grid-template-columns: auto minmax(0, 1fr) auto;
    }
    .vpravo {
      display: none;
    }
    .nahlad {
      width: 88px;
    }
  }
</style>
