<script lang="ts">
  import { app, nastav, skus, toast } from '../lib/app.svelte'
  import { pocet } from '../lib/format'
  import { ikonaZdroja } from '../lib/ikony'
  import type { Polozka, Rozpoznany } from '../lib/typy'
  import Ikona from '../components/Ikona.svelte'
  import Menu from '../components/Menu.svelte'
  import Prazdne from '../components/Prazdne.svelte'
  import Segment from '../components/Segment.svelte'
  import PolozkaFronty from './PolozkaFronty.svelte'

  type Filter = 'vsetko' | 'aktivne' | 'hotove' | 'chyby'

  let format = $state(app.nastavenia?.dl_preset ?? 'video')
  let rozpoznane = $state<Rozpoznany[]>([])
  let filter = $state<Filter>('vsetko')
  let pridava = $state(false)
  // heslo k súborom na úložisku alebo k archívom (zadá sa vopred, platí pre vložené odkazy)
  let sHeslom = $state(false)
  let heslo = $state('')
  let pole: HTMLTextAreaElement | undefined = $state()

  // úložiská (Webshare, Fastshare) majú jeden hotový súbor – formát sa týka len videostránok
  const suborov = $derived(rozpoznane.filter((r) => r.ulozisko).length)
  const videi = $derived(rozpoznane.length - suborov)
  const lenSubory = $derived(suborov > 0 && videi === 0)

  const riadky = $derived(Math.min(6, Math.max(1, app.prikaz.split('\n').length)))

  // zdroje odkazov počas písania
  $effect(() => {
    const text = app.prikaz
    const h = setTimeout(async () => {
      rozpoznane = text.trim() ? ((await skus<Rozpoznany[]>('rozpoznat', text)) ?? []) : []
    }, 180)
    return () => clearTimeout(h)
  })

  const polozky = $derived(app.stav?.polozky ?? [])
  const skupiny = $derived.by(() => {
    const s = {
      stahuje: [] as Polozka[],
      caka: [] as Polozka[],
      chyby: [] as Polozka[],
      hotove: [] as Polozka[],
    }
    for (const p of polozky) {
      if (p.stav === 'stahuje') s.stahuje.push(p)
      else if (p.stav === 'caka' || p.stav === 'pauza') s.caka.push(p)
      else if (p.stav === 'chyba') s.chyby.push(p)
      else s.hotove.push(p)
    }
    s.hotove.reverse()
    return s
  })

  const sekcie = $derived(
    [
      { id: 'stahuje', nazov: 'Sťahuje sa', p: skupiny.stahuje, filtre: ['vsetko', 'aktivne'] },
      { id: 'caka', nazov: 'V poradí', p: skupiny.caka, filtre: ['vsetko', 'aktivne'] },
      { id: 'chyby', nazov: 'Nepodarilo sa', p: skupiny.chyby, filtre: ['vsetko', 'chyby'] },
      { id: 'hotove', nazov: 'Hotové', p: skupiny.hotove, filtre: ['vsetko', 'hotove'] },
    ].filter((s) => s.p.length && s.filtre.includes(filter)),
  )

  const aktivnych = $derived(skupiny.stahuje.length + skupiny.caka.length)
  const pozastavenych = $derived(skupiny.caka.filter((p) => p.stav === 'pauza').length)

  async function stiahnut() {
    if (!app.prikaz.trim() || pridava) return
    pridava = true
    const r = await skus<{ pridane: number; uz_vo_fronte: number; kolekcie: string[] }>(
      'pridat',
      app.prikaz,
      format,
      sHeslom ? heslo : '',
    )
    pridava = false
    if (!r) return
    app.prikaz = ''
    heslo = ''
    sHeslom = false
    if (r.pridane) toast(`Pridané do sťahovania: ${pocet(r.pridane, 'položka', 'položky', 'položiek')}.`, 'ok')
    else if (r.uz_vo_fronte) toast('Tento odkaz už vo fronte je.', 'info')
    if (r.kolekcie.length) {
      app.dialog = { druh: 'playlist', url: r.kolekcie[0] }
      if (r.kolekcie.length > 1) toast('Playlisty a kanály otváram po jednom – ďalšie vlož po tomto.', 'info')
    }
  }

  async function zoSchranky() {
    let text = ''
    try {
      text = await navigator.clipboard.readText()
    } catch {
      text = (await skus<string>('schranka')) ?? ''
    }
    if (!/https?:\/\//.test(text)) {
      toast('V schránke nie je odkaz.', 'info')
      return
    }
    app.prikaz = [app.prikaz.trim(), text.trim()].filter(Boolean).join('\n')
    pole?.focus()
  }

  function prvyOdkaz(kolekcia = false): string {
    const r = rozpoznane.find((x) => !kolekcia || x.kolekcia) ?? rozpoznane[0]
    return r?.url ?? ''
  }

  function klaves(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      stiahnut()
    }
  }
</script>

<div class="stranka">
  <header class="hlavicka">
    <div>
      <h1>Sťahovanie</h1>
      <p class="tlmene">
        {#if aktivnych}
          {[
            skupiny.stahuje.length && pocet(skupiny.stahuje.length, 'súbor sa sťahuje', 'súbory sa sťahujú', 'súborov sa sťahuje'),
            skupiny.caka.length - pozastavenych && pocet(skupiny.caka.length - pozastavenych, 'čaká', 'čakajú', 'čaká'),
            pozastavenych && pocet(pozastavenych, 'pozastavený', 'pozastavené', 'pozastavených'),
          ]
            .filter(Boolean)
            .join(', ')}
        {:else}
          YouTube, Facebook, Instagram, TikTok, Vimeo, Webshare, Fastshare, Datoid a stránky s videom
        {/if}
      </p>
    </div>
  </header>

  <section class="prikaz plocha" aria-label="Nové sťahovanie">
    <div class="vstup">
      <span class="ikona-vstup"><Ikona meno="odkaz" velkost={20} /></span>
      <label class="skryte" for="odkazy">Odkazy na stiahnutie</label>
      <textarea
        id="odkazy"
        bind:this={pole}
        bind:value={app.prikaz}
        rows={riadky}
        placeholder="Vlož odkaz na video, playlist alebo stránku – aj viac naraz"
        onkeydown={klaves}
        spellcheck="false"
        autocomplete="off"
      ></textarea>
    </div>

    {#if rozpoznane.length}
      <ul class="zdroje" aria-label="Rozpoznané odkazy">
        {#each rozpoznane.slice(0, 8) as r (r.url)}
          <li class="stitok" class:akcent={r.kolekcia} title={r.url}>
            <Ikona meno={r.kolekcia ? 'playlist' : ikonaZdroja(r.zdroj)} velkost={14} />
            {r.kolekcia ? `${r.zdroj} – výber videí` : r.zdroj === 'Web' ? 'Stránka' : r.zdroj}
          </li>
        {/each}
        {#if rozpoznane.length > 8}<li class="stitok">a ďalšie ({rozpoznane.length - 8})</li>{/if}
      </ul>
    {/if}

    <div class="ovladanie">
      <div class="pomocne">
        <button class="tl tiche" onclick={zoSchranky}><Ikona meno="schranka" velkost={17} /> Vložiť zo schránky</button>
        <button class="tl tiche" onclick={() => (app.dialog = { druh: 'playlist', url: prvyOdkaz(true) })}>
          <Ikona meno="playlist" velkost={17} /> Playlist alebo kanál
        </button>
        <button class="tl tiche" onclick={() => (app.dialog = { druh: 'skener', url: prvyOdkaz() })}>
          <Ikona meno="skener" velkost={17} /> Prehľadať stránku
        </button>
        <button class="tl tiche" aria-pressed={sHeslom} onclick={() => (sHeslom = !sHeslom)}>
          <Ikona meno="zamok" velkost={17} /> Heslo
        </button>
        {#if sHeslom}
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="pole heslo-pole"
            type="password"
            bind:value={heslo}
            placeholder="Heslo k súboru alebo archívu"
            aria-label="Heslo k súboru alebo archívu"
            autocomplete="off"
            autofocus
          />
        {/if}
      </div>
      <div class="spustit">
        {#if lenSubory}
          <p class="len-subor" id="format-popis">
            <Ikona meno="info" velkost={16} />
            {suborov === 1 ? 'Súbor sa stiahne tak, ako je na úložisku' : 'Súbory sa stiahnu tak, ako sú na úložisku'}
          </p>
        {:else}
          <label class="skryte" for="format">Formát</label>
          <select
            id="format"
            class="vyber"
            bind:value={format}
            onchange={() => nastav({ dl_preset: format })}
            aria-describedby={suborov ? 'format-popis' : undefined}
          >
            <optgroup label="Video">
              {#each app.init?.formaty.filter((f) => f.nazov.startsWith('Video')) ?? [] as f (f.id)}
                <option value={f.id}>{f.nazov}</option>
              {/each}
            </optgroup>
            <optgroup label="Len zvuk">
              {#each app.init?.formaty.filter((f) => f.nazov.startsWith('Zvuk')) ?? [] as f (f.id)}
                <option value={f.id}>{f.nazov}</option>
              {/each}
            </optgroup>
          </select>
        {/if}
        <button class="tl hlavne velke" onclick={stiahnut} disabled={!app.prikaz.trim() || pridava}>
          <Ikona meno="stiahnut" velkost={18} hrubka={2} /> Stiahnuť
        </button>
      </div>
    </div>
    {#if suborov && videi}
      <p class="male tlmene poznamka-formatu" id="format-popis">
        Formát platí pre {pocet(videi, 'video', 'videá', 'videí')}, {pocet(suborov, 'súbor', 'súbory', 'súborov')} z úložísk
        sa {suborov === 1 ? 'stiahne' : 'stiahnu'} bez zmeny.
      </p>
    {/if}
  </section>

  {#if polozky.length}
    <div class="lista">
      <Segment
        popis="Zobraziť"
        bind:hodnota={filter}
        moznosti={[
          { id: 'vsetko', text: 'Všetko', pocet: polozky.length },
          { id: 'aktivne', text: 'Aktívne', pocet: aktivnych },
          { id: 'hotove', text: 'Hotové', pocet: skupiny.hotove.length },
          { id: 'chyby', text: 'Chyby', pocet: skupiny.chyby.length },
        ]}
      />
      <div class="hromadne">
        {#if aktivnych - pozastavenych > 0}
          <button class="tl tiche" onclick={() => skus('pozastavit_vsetko')}><Ikona meno="pauza" velkost={16} /> Pozastaviť všetko</button>
        {/if}
        {#if pozastavenych || skupiny.chyby.length}
          <button class="tl tiche" onclick={() => skus('spustit_vsetko')}><Ikona meno="play" velkost={16} /> Spustiť všetko</button>
        {/if}
        {#if skupiny.hotove.length}
          <button class="tl tiche" onclick={() => skus('vycistit_hotove')}><Ikona meno="vycistit" velkost={16} /> Vyčistiť hotové</button>
        {/if}
        <button class="tl tiche" onclick={() => skus('otvorit')}><Ikona meno="priecinok" velkost={16} /> Priečinok</button>
        <Menu
          popis="Ďalšie možnosti frontu"
          polozky={[
            {
              text: 'Vyčistiť front',
              ikona: 'vycistit',
              akcia: () => {
                if (aktivnych) app.dialog = { druh: 'zmazat', co: 'front' }
                else skus('vycistit_front').then(() => toast('Front je prázdny. Stiahnuté súbory ostali na disku.', 'ok'))
              },
            },
            {
              text: 'Vymazať stiahnuté súbory…',
              ikona: 'kos',
              nebezpecne: true,
              akcia: () => (app.dialog = { druh: 'zmazat', co: 'subory', ids: null }),
            },
          ]}
        />
      </div>
    </div>

    {#each sekcie as s (s.id)}
      <section class="sekcia" aria-labelledby="sekcia-{s.id}">
        <h2 id="sekcia-{s.id}">{s.nazov} <span class="tlmene cisla">{s.p.length}</span></h2>
        <div class="zoznam">
          {#each s.p as p (p.id)}
            <PolozkaFronty {p} />
          {/each}
        </div>
      </section>
    {:else}
      <p class="nic tlmene">V tomto zobrazení nič nie je.</p>
    {/each}
  {:else}
    <div class="prazdne">
      <Prazdne />
      <h2>Front je prázdny</h2>
      <p class="tlmene">
        Vlož odkaz vyššie alebo ho sem pretiahni z prehliadača. Prerušené sťahovanie pokračuje tam, kde skončilo – aj
        po vypnutí počítača.
      </p>
    </div>
  {/if}
</div>

<style>
  .stranka {
    max-width: 1080px;
    padding: var(--s8) var(--s8) var(--s10);
  }
  .hlavicka {
    margin-bottom: var(--s5);
  }
  .hlavicka p {
    margin-top: var(--s1);
  }
  .heslo-pole {
    width: 260px;
    height: 34px;
  }
  .len-subor {
    display: flex;
    align-items: center;
    gap: var(--s2);
    min-height: 44px;
    padding: 0 var(--s3);
    color: var(--ink-2);
  }
  .poznamka-formatu {
    margin: var(--s2) var(--s1) 0;
  }
  .prikaz {
    padding: var(--s3);
    box-shadow: var(--shadow-soft);
  }
  .vstup {
    display: flex;
    align-items: flex-start;
    gap: var(--s2);
    padding: 0 var(--s2);
  }
  .ikona-vstup {
    padding-top: 12px;
    color: var(--accent-ink);
  }
  textarea {
    flex: 1;
    field-sizing: content;
    max-height: 10em;
    min-height: 46px;
    padding: 12px var(--s1);
    border: 0;
    background: transparent;
    color: var(--ink);
    font: 400 var(--t-l) / 1.4 var(--font-text);
    resize: none;
    user-select: text;
  }
  textarea::placeholder {
    color: var(--faint);
  }
  textarea:focus-visible {
    outline: none;
  }
  .prikaz:focus-within {
    border-color: var(--accent-line);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .zdroje {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    list-style: none;
    margin: 0;
    padding: 0 var(--s3) var(--s2) 40px;
  }
  .ovladanie {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    padding-top: var(--s3);
    border-top: 1px solid var(--line);
  }
  .pomocne {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
  }
  .spustit {
    display: flex;
    gap: var(--s2);
    align-items: center;
  }
  .spustit .vyber {
    width: 290px;
    height: 44px;
  }
  .lista {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    margin: var(--s8) 0 var(--s2);
  }
  .hromadne {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
  }
  .sekcia {
    margin-top: var(--s5);
  }
  .sekcia h2 {
    display: flex;
    gap: var(--s2);
    align-items: baseline;
    padding: 0 var(--s3) var(--s1);
    font-size: var(--t-m);
  }
  .sekcia h2 span {
    font-family: var(--font-text);
    font-size: var(--t-s);
    font-weight: 600;
  }
  .zoznam {
    display: flex;
    flex-direction: column;
  }
  .nic {
    padding: var(--s8) var(--s3);
  }
  .prazdne {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--s2);
    padding: var(--s10) var(--s4) var(--s8);
    text-align: center;
  }
  .prazdne h2 {
    margin-top: var(--s4);
    font-size: var(--t-xl);
  }
  .prazdne p {
    max-width: 52ch;
  }
  @media (max-width: 860px) {
    .stranka {
      padding: var(--s5) var(--s4) var(--s8);
    }
    .spustit {
      width: 100%;
    }
    .spustit .vyber {
      flex: 1;
      width: auto;
    }
  }
</style>
