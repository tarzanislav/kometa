<script lang="ts">
  // Kurzy po prihlásení: vlastné okno prehliadača → prihlásenie → načítať lekcie → stiahnuť
  // do priečinkov podľa kapitol. Stránky s kurzami sú v záložkách; prihlasovacie údaje k nim
  // sú v trezore systému a Kometa ich vyplní len na ich stránke.
  import { onMount } from 'svelte'
  import { volaj } from '../lib/api'
  import { app, skus, toast } from '../lib/app.svelte'
  import { pocet } from '../lib/format'
  import type { Kurz, Zalozka } from '../lib/typy'
  import Ikona from '../components/Ikona.svelte'

  let adresa = $state('')
  let kurz = $state<Kurz | null>(null)
  let vybrane = $state<Set<string>>(new Set())
  let nacitava = $state(false)
  let format = $state('video')

  const prehliadac = $derived(app.stav?.prehliadac ?? null)
  const vsetkyLekcie = $derived(kurz?.kapitoly.flatMap((k) => k.lekcie) ?? [])
  // lekcie a materiály kurzu (bez odkazov z menu a päty stránky)
  const kurzoveLekcie = $derived(kurz?.kapitoly.filter((k) => !k.menu).flatMap((k) => k.lekcie) ?? [])
  const materialov = $derived(kurzoveLekcie.filter((l) => l.material).length)

  // ---- moje stránky s kurzami ----
  let zalozky = $state<Zalozka[]>([])
  // upravovaná záložka (nová má prázdne id)
  let uprava = $state<Zalozka | null>(null)
  let noveHeslo = $state('')
  let ukazHeslo = $state(false)

  const host = (u: string) => {
    try {
      return new URL(u).hostname.replace(/^www\./, '').toLowerCase()
    } catch {
      return ''
    }
  }
  const patri = (h: string, z: string) => !!h && !!z && (h === z || h.endsWith('.' + z))
  // záložka otvorenej stránky (najpresnejšia doména)
  const zalozkaStranky = $derived.by(() => {
    const h = prehliadac ? host(prehliadac.adresa) : ''
    return (
      zalozky
        .filter((z) => patri(h, z.host ?? ''))
        .sort((a, b) => (b.host?.length ?? 0) - (a.host?.length ?? 0))[0] ?? null
    )
  })

  onMount(async () => {
    zalozky = (await skus<Zalozka[]>('kurz_zalozky')) ?? []
  })

  function pridatZalozku() {
    const z = prehliadac && !zalozkaStranky ? prehliadac : null
    let povod = adresa.trim()
    try {
      if (z) povod = new URL(z.adresa).origin
    } catch {
      /* nechať adresu z poľa */
    }
    uprava = {
      id: '',
      nazov: z ? z.titulok.split(/[|–]/).pop()?.trim() || '' : '',
      adresa: povod,
      meno: '',
    }
    noveHeslo = ''
    ukazHeslo = false
  }

  function upravitZalozku(z: Zalozka) {
    uprava = { ...z }
    noveHeslo = ''
    ukazHeslo = false
  }

  async function ulozitZalozku() {
    if (!uprava) return
    // pri úprave prázdne heslo = nechať uložené
    const heslo = noveHeslo ? noveHeslo : uprava.id ? null : ''
    const r = await skus<Zalozka[]>('kurz_zalozka_ulozit', uprava, heslo)
    if (!r) return
    zalozky = r
    toast(noveHeslo ? 'Stránka je uložená, heslo je v trezore systému.' : 'Stránka je uložená.', 'ok')
    uprava = null
    noveHeslo = ''
  }

  async function zabudnutHeslo() {
    if (!uprava?.id) return
    const r = await skus<Zalozka[]>('kurz_zalozka_ulozit', uprava, '')
    if (!r) return
    zalozky = r
    uprava = { ...uprava, s_heslom: false }
    toast('Uložené heslo je zmazané.', 'ok')
  }

  async function zmazatZalozku(z: Zalozka) {
    const r = await skus<Zalozka[]>('kurz_zalozka_zmazat', z.id)
    if (!r) return
    zalozky = r
    if (uprava?.id === z.id) uprava = null
  }

  async function prihlasit(ticho = false) {
    let r: { meno: boolean; heslo: boolean; ina_stranka: boolean }
    try {
      r = await volaj('kurz_prihlasit')
    } catch (e) {
      if (!ticho) toast(e instanceof Error ? e.message : String(e), 'chyba')
      return
    }
    if (r.heslo)
      toast(
        r.meno ? 'Meno aj heslo sú vyplnené – klikni na stránke Prihlásiť.' : 'Heslo je vyplnené – doplň meno a prihlás sa.',
        'ok',
      )
    else if (!ticho) toast('Na tejto stránke nie je prihlasovací formulár – prejdi na prihlásenie a skús znova.')
  }

  async function otvoritZalozku(z: Zalozka) {
    adresa = z.adresa
    await skus('kurz_otvorit', z.adresa)
    // keď je otvorená rovno stránka s prihlásením, meno a heslo sa vyplnia samé
    if (z.s_heslom) setTimeout(() => prihlasit(true), 3500)
  }

  // všetky kapitoly kurzu (tie, v ktorých Kometa spoznala lekcie) a všetky materiály; keď
  // nespoznala nič, všetko okrem menu a päty stránky
  function oznacitVsetko() {
    const kapitoly = kurz?.kapitoly.filter((k) => !k.menu) ?? []
    const kurzove = kapitoly.filter((k) => k.lekcie.some((l) => l.odporucana))
    const zdroj = kurzove.length ? kurzove : kapitoly
    vybrane = new Set([
      ...zdroj.flatMap((k) => k.lekcie.map((l) => l.url)),
      ...kapitoly.flatMap((k) => k.lekcie.filter((l) => l.material).map((l) => l.url)),
    ])
  }

  function oznacitOdporucane() {
    vybrane = new Set(vsetkyLekcie.filter((l) => l.odporucana).map((l) => l.url))
  }
  const formaty = $derived((app.init?.formaty ?? []).filter((f) => f.id !== 'jazyky'))

  async function otvorit() {
    if (!adresa.trim()) return
    await skus('kurz_otvorit', adresa.trim())
  }

  async function nacitat() {
    nacitava = true
    const k = await skus<Kurz>('kurz_nacitat')
    nacitava = false
    if (!k) return
    kurz = k
    vybrane = new Set(k.kapitoly.flatMap((x) => x.lekcie.filter((l) => l.odporucana).map((l) => l.url)))
  }

  function prepni(url: string) {
    const s = new Set(vybrane)
    if (s.has(url)) s.delete(url)
    else s.add(url)
    vybrane = s
  }

  function prepniKapitolu(i: number, zapnut: boolean) {
    const s = new Set(vybrane)
    for (const l of kurz!.kapitoly[i].lekcie) {
      if (zapnut) s.add(l.url)
      else s.delete(l.url)
    }
    vybrane = s
  }

  async function stiahnut() {
    if (!kurz) return
    const vyber: Kurz = {
      nazov: kurz.nazov.trim() || 'Kurz',
      adresa: kurz.adresa,
      kapitoly: kurz.kapitoly
        .map((k) => ({ nazov: k.nazov, lekcie: k.lekcie.filter((l) => vybrane.has(l.url)) }))
        .filter((k) => k.lekcie.length),
    }
    const r = await skus<{ pridane: number; uz_vo_fronte: number }>('kurz_stiahnut', vyber, format)
    if (!r) return
    toast(`Do sťahovania: ${pocet(r.pridane, 'lekcia', 'lekcie', 'lekcií')}.`, 'ok')
    app.stranka = 'stahovanie'
  }
</script>

<div class="stranka">
  <header>
    <h1>Kurzy</h1>
    <p class="tlmene">
      Pre kurzy, na ktoré si prihlásený. Otvor portál v okne Komety, prihlás sa a prejdi na stránku kurzu so zoznamom
      lekcií. Kometa z nej načíta lekcie a stiahne ich do priečinkov podľa kapitol.
    </p>
  </header>

  <section class="plocha krok" aria-labelledby="h-krok1">
    <h2 id="h-krok1"><span class="cislo">1</span> Otvor portál a prihlás sa</h2>
    <div class="zalozky">
      <div class="zalozky-hlava">
        <h3 id="h-zalozky">Moje stránky s kurzami</h3>
        <button class="tl tiche" onclick={pridatZalozku}>
          <Ikona meno="plus" velkost={16} />
          {prehliadac && !zalozkaStranky ? 'Pridať otvorenú stránku' : 'Pridať stránku'}
        </button>
      </div>
      {#if zalozky.length}
        <ul class="zoznam-zaloziek" aria-labelledby="h-zalozky">
          {#each zalozky as z (z.id)}
            <li class:aktivna={zalozkaStranky?.id === z.id}>
              <button class="zalozka" onclick={() => otvoritZalozku(z)} title="Otvoriť {z.adresa}">
                <Ikona meno="kurz" velkost={18} />
                <span class="zalozka-text">
                  <strong>{z.nazov}</strong>
                  <span class="tlmene male">{z.host}{z.meno ? ` · ${z.meno}` : ''}</span>
                </span>
                {#if z.s_heslom}
                  <span class="s-heslom" title="Prihlásenie je uložené"><Ikona meno="zamok" velkost={14} /></span>
                {/if}
              </button>
              <button class="tl ikona" title="Upraviť" aria-label="Upraviť {z.nazov}" onclick={() => upravitZalozku(z)}>
                <Ikona meno="nastavenia" velkost={16} />
              </button>
            </li>
          {/each}
        </ul>
      {:else if !uprava}
        <p class="male tlmene">
          Ulož si stránky, kde máš kúpené kurzy – otvoríš ich jedným klikom a Kometa ti na nich vyplní prihlásenie.
        </p>
      {/if}
      {#if uprava}
        <form
          class="uprava plocha"
          aria-label={uprava.id ? 'Upraviť stránku' : 'Nová stránka s kurzami'}
          onsubmit={(e) => {
            e.preventDefault()
            ulozitZalozku()
          }}
        >
          <label class="popis" for="z-nazov">Názov</label>
          <input id="z-nazov" class="pole" bind:value={uprava.nazov} placeholder="Napríklad IT Akadémia" />
          <label class="popis" for="z-adresa">Adresa</label>
          <input id="z-adresa" class="pole" bind:value={uprava.adresa} placeholder="https://www.vita.sk" autocomplete="url" />
          <label class="popis" for="z-meno">Prihlasovacie meno alebo e-mail</label>
          <input id="z-meno" class="pole" bind:value={uprava.meno} autocomplete="off" spellcheck="false" />
          <label class="popis" for="z-heslo">Heslo</label>
          <div class="heslo-pole">
            <input
              id="z-heslo"
              class="pole"
              type={ukazHeslo ? 'text' : 'password'}
              bind:value={noveHeslo}
              autocomplete="new-password"
              placeholder={uprava.s_heslom ? 'Uložené – nechaj prázdne, ak ho nemeníš' : 'Nepovinné'}
            />
            <button type="button" class="tl tiche" onclick={() => (ukazHeslo = !ukazHeslo)} aria-pressed={ukazHeslo}>
              {ukazHeslo ? 'Skryť' : 'Ukázať'}
            </button>
          </div>
          <p class="male tlmene sirka">
            Heslo sa uloží do trezoru systému (vo Windows Správca poverení), nie do súboru Komety. Kometa ho vyplní len na
            tejto stránke – prihlásiť sa klikneš ty.
          </p>
          <div class="akcie-upravy sirka">
            <button class="tl hlavne" disabled={!uprava.adresa.trim()}>Uložiť</button>
            <button type="button" class="tl tiche" onclick={() => (uprava = null)}>Zrušiť</button>
            {#if uprava.id && uprava.s_heslom}
              <button type="button" class="tl tiche" onclick={zabudnutHeslo}>Zmazať uložené heslo</button>
            {/if}
            {#if uprava.id}
              <button type="button" class="tl tiche nebezpecne" onclick={() => uprava && zmazatZalozku(uprava)}>
                <Ikona meno="kos" velkost={16} /> Odstrániť stránku
              </button>
            {/if}
          </div>
        </form>
      {/if}
    </div>
    {#if prehliadac && zalozkaStranky?.s_heslom}
      <div class="prihlasenie">
        <span><Ikona meno="zamok" velkost={16} /> Otvorená je stránka <strong>{zalozkaStranky.nazov}</strong></span>
        <button class="tl" onclick={() => prihlasit()}>Vyplniť prihlásenie</button>
      </div>
    {/if}
    <form
      class="adresa"
      onsubmit={(e) => {
        e.preventDefault()
        otvorit()
      }}
    >
      <label class="skryte" for="kurz-adresa">Iná adresa portálu alebo kurzu</label>
      <input id="kurz-adresa" class="pole" bind:value={adresa} placeholder="https://www.vita.sk" autocomplete="url" />
      <button class="tl hlavne" disabled={!adresa.trim()}><Ikona meno="vonku" velkost={16} /> Otvoriť prehliadač</button>
    </form>
    <p class="male tlmene">
      Prihlásenie ostane len v tomto počítači a Kometa ho pošle iba tejto stránke. Video chránené DRM Kometa nestiahne.
    </p>
  </section>

  <section class="plocha krok" aria-labelledby="h-krok2">
    <h2 id="h-krok2"><span class="cislo">2</span> Načítaj lekcie z otvorenej stránky</h2>
    {#if prehliadac}
      <p class="stranka-prehliadaca">
        <Ikona meno="web" velkost={16} />
        <span><strong>{prehliadac.titulok || 'Stránka'}</strong> <span class="tlmene vyber-text">{prehliadac.adresa}</span></span>
      </p>
      <button class="tl" onclick={nacitat} disabled={nacitava}>
        <Ikona meno="hladat" velkost={16} />
        {nacitava ? 'Načítavam…' : 'Načítať lekcie z tejto stránky'}
      </button>
    {:else}
      <p class="tlmene">Prehliadač je zatvorený – otvor ho v kroku 1.</p>
    {/if}
  </section>

  {#if kurz}
    <section class="plocha krok" aria-labelledby="h-krok3">
      <h2 id="h-krok3"><span class="cislo">3</span> Vyber lekcie a stiahni</h2>
      <div class="nazov-kurzu">
        <label class="popis" for="kurz-nazov">Priečinok kurzu</label>
        <input id="kurz-nazov" class="pole" bind:value={kurz.nazov} />
      </div>
      <div class="vyber-lekcii">
        <button class="tl" onclick={oznacitVsetko}>
          <Ikona meno="fajka" velkost={16} />
          {materialov ? 'Označiť všetky kapitoly a materiály' : 'Označiť všetky kapitoly'}
        </button>
        <button class="tl tiche" onclick={oznacitOdporucane}>Len odporúčané</button>
        <button class="tl tiche" onclick={() => (vybrane = new Set())}>Zrušiť výber</button>
      </div>
      <div class="kapitoly">
        {#each kurz.kapitoly as k, i (i)}
          {@const vsetky = k.lekcie.every((l) => vybrane.has(l.url))}
          {#if k.menu}
            <details class="kapitola menu-kapitola">
              <summary>
                {k.nazov} <span class="tlmene cisla">{k.lekcie.length}</span>
                <span class="tlmene male">– iné kurzy, archívy, bonusy; zvyčajne nepatria ku kurzu</span>
              </summary>
              <ul>
                {#each k.lekcie as l (l.url)}
                  <li>
                    <input
                      type="checkbox"
                      checked={vybrane.has(l.url)}
                      onchange={() => prepni(l.url)}
                      aria-label="Stiahnuť {l.nazov}"
                    />
                    <span class="lekcia">
                      <span>{l.nazov}</span>
                      <span class="tlmene male adresa-lekcie">{l.url}</span>
                    </span>
                  </li>
                {/each}
              </ul>
            </details>
          {:else}
          <fieldset class="kapitola">
            <legend>
              <label>
                <input type="checkbox" checked={vsetky} onchange={() => prepniKapitolu(i, !vsetky)} />
                <span>{k.nazov || 'Bez kapitoly'}</span>
                <span class="tlmene cisla">{k.lekcie.length}</span>
              </label>
            </legend>
            <ul>
              {#each k.lekcie as l (l.url)}
                <li>
                  <input
                    type="checkbox"
                    checked={vybrane.has(l.url)}
                    onchange={() => prepni(l.url)}
                    aria-label="Stiahnuť lekciu {l.nazov}"
                  />
                  <span class="lekcia">
                    <input class="pole male-pole" bind:value={l.nazov} aria-label="Názov lekcie" />
                    <span class="tlmene male adresa-lekcie">
                      {#if l.material}<span class="stitok">Materiál</span>{/if}
                      {#if l.dlzka}<span class="cisla">{l.dlzka}</span> ·{/if}
                      {l.url}
                    </span>
                  </span>
                </li>
              {/each}
            </ul>
          </fieldset>
          {/if}
        {/each}
      </div>
      <div class="pata">
        <label class="skryte" for="kurz-format">Formát</label>
        <select id="kurz-format" class="vyber" bind:value={format}>
          {#each formaty as f (f.id)}<option value={f.id}>{f.nazov}</option>{/each}
        </select>
        <button class="tl hlavne velke" disabled={!vybrane.size} onclick={stiahnut}>
          <Ikona meno="stiahnut" velkost={18} hrubka={2} /> Stiahnuť {pocet(vybrane.size, 'lekciu', 'lekcie', 'lekcií')}
        </button>
      </div>
      <p class="male tlmene">
        Vybraté: {vybrane.size} z {kurzoveLekcie.length}{materialov ? ` (z toho materiálov ${materialov})` : ''}. Každá lekcia dostane číslo podľa poradia na stránke, kapitoly
        vlastný priečinok.
      </p>
    </section>
  {/if}
</div>

<style>
  .stranka {
    max-width: 1000px;
    padding: var(--s8) var(--s8) var(--s10);
  }
  header p {
    margin-top: var(--s1);
    max-width: 75ch;
  }
  .krok {
    display: grid;
    gap: var(--s3);
    margin-top: var(--s5);
    padding: var(--s5);
  }
  .krok h2 {
    display: flex;
    align-items: center;
    gap: var(--s2);
    font-size: var(--t-l);
  }
  .cislo {
    display: inline-grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    background: var(--accent);
    color: var(--on-accent);
    font-size: var(--t-s);
  }
  .adresa {
    display: flex;
    gap: var(--s2);
  }
  .stranka-prehliadaca {
    display: flex;
    align-items: center;
    gap: var(--s2);
    overflow: hidden;
  }
  .stranka-prehliadaca span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .krok > .tl {
    justify-self: start;
  }
  .nazov-kurzu {
    max-width: 560px;
  }
  .kapitoly {
    display: grid;
    gap: var(--s3);
    max-height: 520px;
    overflow: auto;
    padding-right: var(--s2);
  }
  .kapitola {
    border: 1px solid var(--line);
    border-radius: var(--r-ctl);
    padding: var(--s2) var(--s3) var(--s3);
  }
  .kapitola legend label {
    display: flex;
    align-items: center;
    gap: var(--s2);
    font-weight: 600;
  }
  .kapitola ul {
    display: grid;
    gap: var(--s1);
    list-style: none;
    padding: 0;
    margin-top: var(--s2);
  }
  .kapitola li {
    display: flex;
    align-items: center;
    gap: var(--s2);
  }
  .lekcia {
    display: grid;
    flex: 1;
    min-width: 0;
  }
  .male-pole {
    height: 30px;
  }
  .adresa-lekcie {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .zalozky {
    display: grid;
    gap: var(--s2);
  }
  .zalozky-hlava {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
  }
  .zalozky-hlava h3 {
    font-size: var(--t-m);
  }
  .zoznam-zaloziek {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: var(--s2);
    list-style: none;
    padding: 0;
  }
  .zoznam-zaloziek li {
    display: flex;
    align-items: center;
    gap: var(--s1);
    border: 1px solid var(--line);
    border-radius: var(--r-ctl);
    padding-right: var(--s1);
  }
  .zoznam-zaloziek li.aktivna {
    border-color: var(--accent);
  }
  .zalozka {
    display: flex;
    flex: 1;
    min-width: 0;
    align-items: center;
    gap: var(--s2);
    padding: var(--s2) var(--s3);
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
    border-radius: var(--r-ctl);
  }
  .zalozka:hover {
    background: var(--accent-soft);
  }
  .zalozka-text {
    display: grid;
    min-width: 0;
  }
  .zalozka-text span,
  .zalozka-text strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .s-heslom {
    color: var(--accent);
  }
  .uprava {
    display: grid;
    gap: var(--s1);
    max-width: 560px;
    padding: var(--s4);
  }
  .uprava .pole {
    width: 100%;
  }
  .uprava .popis:not(:first-child) {
    margin-top: var(--s2);
  }
  .uprava .sirka {
    margin-top: var(--s2);
  }
  .heslo-pole {
    display: flex;
    gap: var(--s2);
  }
  .heslo-pole .pole {
    flex: 1;
  }
  .akcie-upravy {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
  }
  .prihlasenie > span {
    display: inline-flex;
    align-items: center;
    gap: var(--s2);
  }
  .prihlasenie {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--s2);
    padding: var(--s2) var(--s3);
    border-radius: var(--r-ctl);
    background: var(--accent-soft);
  }
  .vyber-lekcii {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
  }
  .menu-kapitola summary {
    cursor: pointer;
    font-weight: 600;
  }
  .stitok {
    margin-right: var(--s1);
  }
  .pata {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
    align-items: center;
  }
  .pata .vyber {
    width: 300px;
  }
</style>
