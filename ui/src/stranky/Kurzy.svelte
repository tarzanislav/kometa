<script lang="ts">
  // Kurzy po prihlásení: vlastné okno prehliadača → prihlásenie → načítať lekcie → stiahnuť
  // do priečinkov podľa kapitol. Stránky s kurzami sú v záložkách; prihlasovacie údaje k nim
  // sú v trezore systému a Kometa ich vyplní len na ich stránke.
  import { onMount } from 'svelte'
  import { volaj } from '../lib/api'
  import { app, nastav, skus, toast } from '../lib/app.svelte'
  import { pocet } from '../lib/format'
  import type { Kurz, OdkazMaterialu, PrechodOsnovy, StiahnutyKurz, VybranyKurz, Zalozka } from '../lib/typy'
  import Ikona from '../components/Ikona.svelte'

  let adresa = $state('')
  let kurz = $state<Kurz | null>(null)
  let vybrane = $state<Set<string>>(new Set())
  let nacitava = $state(false)
  // formát a šetrné sťahovanie si Kometa pamätá
  let format = $state(app.nastavenia?.kurzy_format ?? 'video')
  const setrne = $derived(app.nastavenia?.kurzy_setrne ?? true)

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

  // ---- vybrané kurzy (konkrétne stránky kurzov) a stiahnuté kurzy ----
  let vybraneKurzy = $state<VybranyKurz[]>([])
  let oznaceneKurzy = $state<Set<string>>(new Set())
  let potvrditVsetky = $state(false)
  let stiahnute = $state<StiahnutyKurz[]>([])

  // odkazy v textových súboroch s materiálmi (podľa priečinka kurzu)
  let odkazy = $state<Record<string, OdkazMaterialu[]>>({})

  async function nacitatStiahnute() {
    stiahnute = (await skus<StiahnutyKurz[]>('kurzy_stiahnute')) ?? stiahnute
    const nove: Record<string, OdkazMaterialu[]> = {}
    for (const k of stiahnute) {
      nove[k.priecinok] = (await skus<OdkazMaterialu[]>('kurz_odkazy_materialov', k.priecinok)) ?? []
    }
    odkazy = nove
  }

  // odkaz z materiálov v prehliadači kurzov (tam si prihlásený); čo sa tam stiahne, ide do MATERIALY
  async function otvoritOdkaz(k: StiahnutyKurz, o: OdkazMaterialu) {
    try {
      await volaj('kurz_otvorit', o.odkaz, { priecinok: k.priecinok })
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), 'chyba')
      return
    }
    toast(`Odkaz je otvorený v prehliadači kurzov. Čo tam stiahneš, uloží sa do ${k.nazov} › MATERIALY.`, 'ok')
  }

  onMount(() => {
    skus<Zalozka[]>('kurz_zalozky').then((r) => (zalozky = r ?? []))
    skus<VybranyKurz[]>('kurz_vybrane').then((r) => (vybraneKurzy = r ?? []))
    nacitatStiahnute()
    // stiahnuté lekcie pribúdajú – zoznam sa raz za čas obnoví
    const t = window.setInterval(() => !document.hidden && nacitatStiahnute(), 15000)
    return () => clearInterval(t)
  })

  const bezZnacky = (u: string) => u.replace(/#kometa-lekcia-\d+$/, '').replace(/\/+$/, '')
  // otvorená stránka je už vo Vybraných?
  const vybranyStranky = $derived.by(() => {
    const a = prehliadac ? bezZnacky(prehliadac.adresa) : ''
    return a ? (vybraneKurzy.find((k) => bezZnacky(k.adresa) === a) ?? null) : null
  })

  async function pridatDoVybranych() {
    if (!prehliadac) return
    // názov: načítaný kurz z tejto stránky, inak titulok stránky
    const nazov = kurz && bezZnacky(kurz.adresa) === bezZnacky(prehliadac.adresa) ? kurz.nazov : prehliadac.titulok
    const r = await skus<VybranyKurz[]>('kurz_vybrany_ulozit', nazov, prehliadac.adresa)
    if (!r) return
    vybraneKurzy = r
    toast('Kurz je vo Vybraných kurzoch – nabudúce ho otvoríš jedným klikom.', 'ok')
  }

  async function otvoritVybrany(k: VybranyKurz) {
    adresa = k.adresa
    await skus('kurz_otvorit', k.adresa, { nazov: k.nazov })
    // keď portál najprv ukáže prihlásenie, meno a heslo sa vyplnia samé
    if (k.s_heslom) setTimeout(() => prihlasit(true), 3500)
  }

  function prepniKurz(id: string) {
    const s = new Set(oznaceneKurzy)
    if (s.has(id)) s.delete(id)
    else s.add(id)
    oznaceneKurzy = s
  }

  async function odstranitKurzy(ids: string[] | null) {
    const r = await skus<VybranyKurz[]>('kurz_vybrane_zmazat', ids)
    if (!r) return
    vybraneKurzy = r
    oznaceneKurzy = new Set([...oznaceneKurzy].filter((id) => r.some((k) => k.id === id)))
    potvrditVsetky = false
  }

  async function otvoritPrehlad(k: StiahnutyKurz) {
    if (k.prehlad) await skus('kurz_prehlad_otvorit', k.prehlad)
  }

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

  // ---- prechod osnovy: lekcie, ktoré sa otvárajú kliknutím na tej istej stránke ----
  let prechod = $state<PrechodOsnovy | null>(null)
  let casovacPrechodu = 0
  // načítané lekcie (bez materiálov a menu) vs. riadky osnovy na stránke
  const nacitanychLekcii = $derived(
    kurz?.kapitoly.filter((k) => !k.menu).flatMap((k) => k.lekcie.filter((l) => l.odporucana && !l.material)).length ?? 0,
  )
  const chybajuLekcie = $derived(
    !!kurz && !kurz.kapitoly.some((k) => k.lekcie.some((l) => l.druh)) && (kurz.riadkov_osnovy ?? 0) > nacitanychLekcii + 1,
  )

  async function sledujPrechod() {
    clearTimeout(casovacPrechodu)
    const r = await skus<{ stav: PrechodOsnovy; kurz: Kurz | null }>('kurz_prechod')
    if (!r) return
    prechod = r.stav
    if (r.stav.bezi) {
      casovacPrechodu = window.setTimeout(sledujPrechod, 700)
      return
    }
    if (r.stav.chyba) {
      toast(r.stav.chyba, 'chyba')
      return
    }
    if (r.kurz) {
      const stary = kurz
      kurz = { ...r.kurz, nazov: stary?.nazov || r.kurz.nazov }
      // materiály z bežného načítania (záložka Materiály) ostanú
      const materialy = (stary?.kapitoly ?? []).filter((k) => !k.menu && k.lekcie.every((l) => l.material))
      if (materialy.length) kurz.kapitoly = [...kurz.kapitoly, ...materialy]
      vybrane = new Set(kurz.kapitoly.flatMap((x) => x.lekcie.filter((l) => l.odporucana).map((l) => l.url)))
      const n = kurz.kapitoly.flatMap((k) => k.lekcie).filter((l) => l.druh && l.druh !== 'material').length
      toast(`Osnova je prejdená: ${pocet(n, 'lekcia', 'lekcie', 'lekcií')}.`, 'ok')
    }
  }

  async function prejst() {
    const nazov = kurz?.nazov ?? prehliadac?.titulok ?? ''
    if ((await skus('kurz_prejst', nazov)) === undefined) return
    prechod = { bezi: true, hotovo: 0, celkom: 0, lekcia: '', chyba: '', hotovy: false }
    sledujPrechod()
  }

  async function zastavitPrechod() {
    await skus('kurz_prechod_zastavit')
  }

  // adresa pri lekcii: pri videu z prechodu prehrávač, inak stránka (bez vnútornej značky Komety)
  const adresaLekcie = (l: { url: string; zdroj?: string }) => l.zdroj || l.url.replace(/#kometa-lekcia-\d+$/, '')

  const DRUHY: Record<string, string> = {
    video: 'Video',
    text: 'Text',
    test: 'Test',
    material: 'Materiál',
    nezistene: 'Nedá sa otvoriť',
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
    // čísla kapitol a lekcií podľa stránky: pri sťahovaní časti kurzu (aj neskôr ďalšej časti)
    // sedia názvy súborov a priečinkov s osnovou
    const vyber: Kurz = {
      nazov: kurz.nazov.trim() || 'Kurz',
      adresa: kurz.adresa,
      kapitol: kurz.kapitoly.filter((k) => !k.menu && k.lekcie.length).length,
      kapitoly: kurz.kapitoly
        .map((k, i) => ({
          nazov: k.nazov,
          menu: k.menu,
          cislo: i + 1,
          lekcie: k.lekcie.map((l, j) => ({ ...l, cislo: j + 1 })).filter((l) => vybrane.has(l.url)),
        }))
        .filter((k) => k.lekcie.length),
    }
    const r = await skus<{ pridane: number; uz_vo_fronte: number; texty?: number; prehlad?: string | null }>(
      'kurz_stiahnut',
      vyber,
      format,
      { setrne },
    )
    if (!r) return
    const prehlad = r.prehlad
    toast(
      `Do sťahovania: ${pocet(r.pridane, 'lekcia', 'lekcie', 'lekcií')}${setrne ? ' (šetrne)' : ''}.` +
        (r.texty ? ` Uložené texty a testy: ${r.texty}.` : '') +
        (prehlad ? ' V priečinku kurzu je Prehľad kurzu.' : ''),
      'ok',
      prehlad ? { text: 'Otvoriť prehľad', spusti: () => skus('kurz_prehlad_otvorit', prehlad) } : undefined,
    )
    nacitatStiahnute()
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
    {#if vybraneKurzy.length}
      <div class="zalozky">
        <div class="zalozky-hlava">
          <h3 id="h-vybrane">Vybrané kurzy</h3>
          <div class="akcie-vybranych">
            {#if oznaceneKurzy.size}
              <button class="tl tiche nebezpecne" onclick={() => odstranitKurzy([...oznaceneKurzy])}>
                <Ikona meno="kos" velkost={16} /> Odstrániť označené ({oznaceneKurzy.size})
              </button>
            {/if}
            {#if potvrditVsetky}
              <span class="male">Odstrániť všetky vybrané kurzy ({vybraneKurzy.length})?</span>
              <button class="tl tiche nebezpecne" onclick={() => odstranitKurzy(null)}>Áno, odstrániť</button>
              <button class="tl tiche" onclick={() => (potvrditVsetky = false)}>Nie</button>
            {:else}
              <button class="tl tiche" onclick={() => (potvrditVsetky = true)}>Odstrániť všetky</button>
            {/if}
          </div>
        </div>
        <ul class="zoznam-zaloziek vybrane-kurzy" aria-labelledby="h-vybrane">
          {#each vybraneKurzy as k (k.id)}
            <li class:aktivna={vybranyStranky?.id === k.id}>
              <input
                type="checkbox"
                checked={oznaceneKurzy.has(k.id)}
                onchange={() => prepniKurz(k.id)}
                aria-label="Označiť kurz {k.nazov}"
              />
              <button class="zalozka" onclick={() => otvoritVybrany(k)} title="Otvoriť {k.adresa}">
                <Ikona meno="kurz" velkost={18} />
                <span class="zalozka-text">
                  <strong>{k.nazov}</strong>
                  <span class="tlmene male">{k.stranka || k.host}</span>
                </span>
                {#if k.s_heslom}
                  <span class="s-heslom" title="Prihlásenie sa vyplní samo"><Ikona meno="zamok" velkost={14} /></span>
                {/if}
              </button>
              <button
                class="tl ikona"
                title="Odstrániť z Vybraných"
                aria-label="Odstrániť {k.nazov} z Vybraných kurzov"
                onclick={() => odstranitKurzy([k.id])}
              >
                <Ikona meno="zavriet" velkost={16} />
              </button>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
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
      <div class="stranka-riadok">
        <p class="stranka-prehliadaca">
          <Ikona meno="web" velkost={16} />
          <span><strong>{prehliadac.titulok || 'Stránka'}</strong> <span class="tlmene vyber-text">{prehliadac.adresa}</span></span>
        </p>
        {#if vybranyStranky}
          <span class="male tlmene ulozeny"><Ikona meno="fajka" velkost={14} /> Vo Vybraných kurzoch</span>
        {:else}
          <button class="tl tiche" onclick={pridatDoVybranych} title="Nabudúce otvoríš tento kurz jedným klikom">
            <Ikona meno="plus" velkost={16} /> Pridať do Vybraných kurzov
          </button>
        {/if}
      </div>
      <div class="nacitanie">
        <button class="tl" onclick={nacitat} disabled={nacitava || !!prechod?.bezi}>
          <Ikona meno="hladat" velkost={16} />
          {nacitava ? 'Načítavam…' : 'Načítať lekcie z tejto stránky'}
        </button>
        <button class="tl" class:hlavne={chybajuLekcie} onclick={prejst} disabled={nacitava || !!prechod?.bezi}>
          <Ikona meno="playlist" velkost={16} /> Prejsť lekcie v osnove
        </button>
      </div>
      {#if app.stav?.prehliadac_materialy}
        <p class="male tlmene">
          Čo stiahneš priamo v prehliadači kurzov (napr. ZIP so zdieľanými materiálmi), uloží sa do
          <span class="vyber-text">{app.stav.prehliadac_materialy}</span>; rovnaký názov dostane na koniec číslo.
        </p>
      {/if}
      <p class="male tlmene">
        <strong>Prejsť lekcie v osnove</strong> – pre portály, kde sa lekcie otvárajú kliknutím na tej istej stránke. Kometa
        v okne prehliadača postupne otvorí každú lekciu a zapíše si video, text, test aj materiály (pár sekúnd na lekciu;
        stránka si ich môže označiť ako pozreté).
      </p>
      {#if chybajuLekcie && !prechod?.bezi}
        <p class="upozornenie" role="status">
          Osnova na stránke má {pocet(kurz?.riadkov_osnovy ?? 0, 'lekciu', 'lekcie', 'lekcií')}, ako samostatné odkazy je z nich
          len {nacitanychLekcii}. Lekcie sa tu otvárajú kliknutím – klikni na <strong>Prejsť lekcie v osnove</strong>.
        </p>
      {/if}
      {#if prechod?.bezi}
        <div class="prechod" role="status">
          <div class="prechod-text">
            <span>
              Prechádzam lekcie: <strong class="cisla">{prechod.hotovo + 1} z {prechod.celkom || '…'}</strong>
              {#if prechod.lekcia}<span class="tlmene"> – {prechod.lekcia}</span>{/if}
            </span>
            <button class="tl tiche" onclick={zastavitPrechod}>Zastaviť</button>
          </div>
          <progress max={prechod.celkom || 1} value={prechod.hotovo}></progress>
        </div>
      {/if}
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
                      {#if l.druh && l.druh !== 'material'}<span class="stitok druh-{l.druh}">{DRUHY[l.druh] ?? l.druh}</span>{/if}
                      {#if l.material}<span class="stitok">Materiál</span>{/if}
                      {#if l.dlzka}<span class="cisla">{l.dlzka}</span> ·{/if}
                      {adresaLekcie(l)}
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
        <select id="kurz-format" class="vyber" bind:value={format} onchange={() => nastav({ kurzy_format: format })}>
          {#each formaty as f (f.id)}<option value={f.id}>{f.nazov}</option>{/each}
        </select>
        <label class="volba-setrne">
          <input type="checkbox" checked={setrne} onchange={(e) => nastav({ kurzy_setrne: e.currentTarget.checked })} />
          Sťahovať šetrne
        </label>
        <button class="tl hlavne velke" disabled={!vybrane.size} onclick={stiahnut}>
          <Ikona meno="stiahnut" velkost={18} hrubka={2} /> Stiahnuť {pocet(vybrane.size, 'lekciu', 'lekcie', 'lekcií')}
        </button>
      </div>
      <p class="male tlmene">
        Vybraté: {vybrane.size} z {kurzoveLekcie.length}{materialov ? ` (z toho materiálov ${materialov})` : ''}. Kurz sa uloží do
        priečinka <strong>KURZY</strong> › <strong>{kurz.nazov.trim() || 'Kurz'}</strong>, každá kapitola do vlastného priečinka
        a lekcia dostane číslo podľa poradia na stránke. V priečinku kurzu bude aj <strong>Prehľad kurzu</strong> – stránka,
        v ktorej si kurz prezrieš a pustíš aj bez internetu.
      </p>
      {#if setrne}
        <p class="male tlmene">
          Šetrne: jedna lekcia naraz, najviac {app.nastavenia?.setrne_rychlost ?? 2} MB/s, jedno spojenie na server a medzi
          videami prestávka okolo {app.nastavenia?.setrne_pauza ?? 45} s – domáca sieť ostane voľná a stránka kurzu nezaťažená.
          Zmeníš v Nastaveniach.
        </p>
      {/if}
    </section>
  {/if}

  {#if stiahnute.length}
    <section class="plocha krok" aria-labelledby="h-stiahnute">
      <h2 id="h-stiahnute">Stiahnuté kurzy</h2>
      <ul class="stiahnute">
        {#each stiahnute as k (k.priecinok)}
          <li class:s-odkazmi={odkazy[k.priecinok]?.length}>
            <span class="zalozka-text">
              <strong>{k.nazov}</strong>
              <span class="tlmene male cisla">
                {k.stiahnutych === k.lekcii
                  ? pocet(k.lekcii, 'lekcia', 'lekcie', 'lekcií')
                  : `stiahnuté ${k.stiahnutych} z ${k.lekcii}`}
              </span>
            </span>
            {#if k.prehlad}
              <button class="tl" onclick={() => otvoritPrehlad(k)}><Ikona meno="play" velkost={16} /> Otvoriť prehľad</button>
              <button class="tl tiche ikona" title="Otvoriť priečinok kurzu" aria-label="Otvoriť priečinok kurzu {k.nazov}"
                onclick={() => skus('otvorit_subor', k.prehlad)}><Ikona meno="priecinok" velkost={16} /></button>
            {/if}
            {#if odkazy[k.priecinok]?.length}
              <div class="odkazy-materialov">
                <span class="male tlmene">Materiály s odkazom (v priečinku MATERIALY):</span>
                {#each odkazy[k.priecinok] as o, i (i)}
                  <div class="odkaz-materialu">
                    <span class="male vyber-text" title={o.subor}>{o.subor}</span>
                    <button class="tl tiche" onclick={() => otvoritOdkaz(k, o)}
                      title="Otvorí sa v prehliadači kurzov, kde si prihlásený">
                      <Ikona meno="vonku" velkost={15} /> Otvoriť v prehliadači kurzov{o.host ? ` (${o.host})` : ''}
                    </button>
                  </div>
                {/each}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
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
  .nacitanie {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
  }
  .upozornenie {
    padding: var(--s2) var(--s3);
    border-left: 3px solid var(--warn);
    border-radius: var(--r-ctl);
    background: var(--accent-soft);
  }
  .prechod {
    display: grid;
    gap: var(--s2);
  }
  .prechod-text {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--s2);
  }
  .prechod progress {
    width: 100%;
    height: 8px;
    accent-color: var(--accent);
  }
  .druh-nezistene {
    color: var(--warn);
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
  .volba-setrne {
    display: inline-flex;
    align-items: center;
    gap: var(--s2);
    margin-right: auto;
    cursor: pointer;
  }
  .akcie-vybranych {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s2);
  }
  .vybrane-kurzy li {
    padding-left: var(--s2);
  }
  .stranka-riadok {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--s2) var(--s3);
  }
  .stranka-riadok .stranka-prehliadaca {
    flex: 1;
    min-width: 0;
  }
  .ulozeny {
    display: inline-flex;
    align-items: center;
    gap: var(--s1);
  }
  .stiahnute {
    display: grid;
    gap: var(--s2);
    list-style: none;
    padding: 0;
  }
  .stiahnute li {
    display: flex;
    align-items: center;
    gap: var(--s2);
    padding: var(--s2) var(--s3);
    border: 1px solid var(--line);
    border-radius: var(--r-ctl);
  }
  .stiahnute .zalozka-text {
    flex: 1;
  }
  .stiahnute li.s-odkazmi {
    flex-wrap: wrap;
  }
  .odkazy-materialov {
    display: grid;
    gap: var(--s1);
    flex-basis: 100%;
    padding-top: var(--s2);
    border-top: 1px dashed var(--line);
  }
  .odkaz-materialu {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--s2);
  }
</style>
