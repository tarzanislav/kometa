<script lang="ts">
  // Odkazy zachytené zo schránky (Ctrl+C) – stiahnuť, otvoriť výber z playlistu alebo zahodiť.
  // Heslá k súborom a archívom sa dajú zadať hneď tu – jednému odkazu, skupine skopírovanej naraz
  // alebo všetkým označeným; po stiahnutí sa archívy rozbalia samé.
  import { app, nastav, skus, toast } from '../lib/app.svelte'
  import { pocet } from '../lib/format'
  import { ikonaZdroja } from '../lib/ikony'
  import type { Zachyteny } from '../lib/typy'
  import Ikona from '../components/Ikona.svelte'
  import Prepinac from '../components/Prepinac.svelte'
  import Segment from '../components/Segment.svelte'

  const n = $derived(app.nastavenia!)
  const zoznam = $derived([...(app.stav?.zachytene ?? [])].reverse())
  let format = $state(app.nastavenia?.strazca_format ?? app.nastavenia?.dl_preset ?? 'video')

  const videa = $derived(zoznam.filter((z) => !z.kolekcia))
  const formaty = $derived(app.init?.formaty ?? [])

  // výber viacerých odkazov (len tie, ktoré v zozname ešte sú)
  let vybrane = $state<string[]>([])
  const oznacene = $derived(vybrane.filter((id) => zoznam.some((z) => z.id === id)))
  const vsetkyOznacene = $derived(zoznam.length > 0 && oznacene.length === zoznam.length)
  let hromadneHeslo = $state('')
  // položka s otvoreným poľom na heslo
  let upravuje = $state<string | null>(null)
  let heslo = $state('')

  // odkazy skopírované naraz spolu, najnovšie hore
  const skupiny = $derived.by(() => {
    const out: { id: string; polozky: Zachyteny[] }[] = []
    for (const z of zoznam) {
      const k = z.skupina || z.id
      const s = out.find((x) => x.id === k)
      if (s) s.polozky.unshift(z)
      else out.push({ id: k, polozky: [z] })
    }
    // v skupine v poradí, v akom boli v skopírovanom texte
    return out
  })

  function prepni(id: string) {
    vybrane = oznacene.includes(id) ? oznacene.filter((x) => x !== id) : [...oznacene, id]
  }

  function prepniViac(ids: string[], zapnut: boolean) {
    vybrane = zapnut ? [...new Set([...oznacene, ...ids])] : oznacene.filter((x) => !ids.includes(x))
  }

  async function priraditHeslo(ids: string[], h: string): Promise<boolean> {
    const n = await skus<number>('schranka_heslo', ids, h)
    if (n == null) return false
    toast(
      h.trim()
        ? `Heslo je priradené: ${pocet(n, 'odkaz', 'odkazy', 'odkazov')}. Archívy sa po stiahnutí rozbalia samé.`
        : 'Heslo je odstránené.',
      'ok',
    )
    return true
  }

  async function hromadne() {
    if (hromadneHeslo.trim() && (await priraditHeslo(oznacene, hromadneHeslo))) hromadneHeslo = ''
  }

  function upravit(z: Zachyteny) {
    upravuje = upravuje === z.id ? null : z.id
    heslo = z.heslo ?? ''
  }

  async function ulozitHeslo(ids: string[], h: string) {
    if (await priraditHeslo(ids, h)) upravuje = null
  }

  async function stiahnut(ids: string[] | null) {
    const r = await skus<{ pridane: number; kolekcie: string[] }>('schranka_stiahnut', ids, format)
    if (!r) return
    vybrane = []
    if (r.pridane) toast(`Pridané do sťahovania: ${pocet(r.pridane, 'položka', 'položky', 'položiek')}.`, 'ok')
    if (r.kolekcie.length) app.dialog = { druh: 'playlist', url: r.kolekcie[0] }
  }

  function vybrat(z: Zachyteny) {
    skus('schranka_zahodit', [z.id])
    app.dialog = { druh: 'playlist', url: z.url }
  }

  // práve zachytené: zvýrazniť a ukázať prvý z nich; zvýraznenie po chvíli zmizne
  $effect(() => {
    const nove = app.noveZoSchranky
    if (!nove.length) return
    const h = setTimeout(() => {
      document.querySelector(`[data-zachyteny="${nove[0]}"]`)?.scrollIntoView({ block: 'center', behavior: 'smooth' })
    }, 60)
    const k = setTimeout(() => (app.noveZoSchranky = []), 6000)
    return () => {
      clearTimeout(h)
      clearTimeout(k)
    }
  })

  function cas(t: number) {
    return new Date(t * 1000).toLocaleTimeString('sk-SK', { hour: '2-digit', minute: '2-digit' })
  }
</script>

<div class="stranka">
  <header>
    <h1>Zo schránky</h1>
    <p class="tlmene">
      Keď skopíruješ odkaz na YouTube, Webshare, Fastshare, Datoid alebo inú známu stránku, Kometa ho zachytí.
    </p>
  </header>

  <section class="plocha nastavenie" aria-label="Strážca schránky">
    <Prepinac
      nazov="Sledovať schránku"
      zapnute={n.strazca ?? false}
      popis="Zachytí sa len to, čo skopíruješ po zapnutí. Bežný text a iné stránky si nevšíma."
      zmena={(v) => nastav({ strazca: v })}
    />
    <div class="rezim" class:zakazane={!n.strazca}>
      <div>
        <p class="nazov">Po skopírovaní</p>
        <p class="male tlmene">
          Playlisty a kanály vždy počkajú tu – vyberieš si videá. Súbory z úložísk sa stiahnu bez zmeny.
        </p>
      </div>
      <Segment
        popis="Čo urobiť po skopírovaní odkazu"
        hodnota={n.strazca_rezim ?? 'zachytit'}
        zmena={(v) => nastav({ strazca_rezim: v })}
        moznosti={[
          { id: 'zachytit', text: 'Len zachytiť' },
          { id: 'stiahnut', text: 'Hneď stiahnuť' },
        ]}
      />
    </div>
    <div class="rezim" class:zakazane={!n.strazca}>
      <Prepinac
        nazov="Ukázať zachytené odkazy"
        zapnute={n.strazca_ukazat ?? true}
        popis="Keď Kometa zachytí odkaz, prepne sa sem priamo k nemu (nie, keď práve píšeš v Komete)."
        zmena={(v) => nastav({ strazca_ukazat: v })}
      />
    </div>
    {#if n.strazca_rezim === 'stiahnut'}
      <div class="rezim">
        <label class="nazov" for="strazca-format">Formát pre videá</label>
        <select
          id="strazca-format"
          class="vyber"
          value={n.strazca_format ?? 'video'}
          onchange={(e) => nastav({ strazca_format: (e.currentTarget as HTMLSelectElement).value })}
        >
          {#each formaty as f (f.id)}<option value={f.id}>{f.nazov}</option>{/each}
        </select>
      </div>
    {/if}
  </section>

  {#if zoznam.length}
    <div class="lista">
      <h2>Zachytené <span class="tlmene cisla">{zoznam.length}</span></h2>
      <div class="akcie">
        <label class="skryte" for="schranka-format">Formát</label>
        <select id="schranka-format" class="vyber" bind:value={format}>
          {#each formaty as f (f.id)}<option value={f.id}>{f.nazov}</option>{/each}
        </select>
        {#if videa.length}
          <button class="tl hlavne" onclick={() => stiahnut(videa.map((z) => z.id))}>
            <Ikona meno="stiahnut" velkost={16} /> Stiahnuť všetko ({videa.length})
          </button>
        {/if}
        <button class="tl tiche" onclick={() => skus('schranka_zahodit', null)}>
          <Ikona meno="vycistit" velkost={16} /> Zahodiť všetko
        </button>
      </div>
    </div>
    <p class="male tlmene rada">
      Heslo k archívom zadaj hneď tu (ikona zámku) – po stiahnutí sa rozbalia samé. Keď je heslo v skopírovanom
      texte („Heslo: …“), Kometa ho doplní sama.
    </p>
    <div class="vyber-riadok">
      <label class="oznacit">
        <input
          type="checkbox"
          checked={vsetkyOznacene}
          onchange={(e) => prepniViac(zoznam.map((z) => z.id), e.currentTarget.checked)}
        />
        Označiť všetko
      </label>
      {#if oznacene.length}
        <form
          class="hromadne"
          onsubmit={(e) => {
            e.preventDefault()
            hromadne()
          }}
        >
          <span class="cisla pocet-oznacenych">Označené: {oznacene.length}</span>
          <label class="skryte" for="hromadne-heslo">Heslo pre označené odkazy</label>
          <input
            id="hromadne-heslo"
            class="pole"
            type="text"
            autocomplete="off"
            spellcheck="false"
            bind:value={hromadneHeslo}
            placeholder="Jedno heslo pre všetky označené"
          />
          <button class="tl" disabled={!hromadneHeslo.trim()}><Ikona meno="zamok" velkost={16} /> Priradiť heslo</button>
          <button type="button" class="tl hlavne" onclick={() => stiahnut(oznacene)}>
            <Ikona meno="stiahnut" velkost={16} /> Stiahnuť označené
          </button>
          <button type="button" class="tl tiche" onclick={() => skus('schranka_zahodit', oznacene)}>
            <Ikona meno="vycistit" velkost={16} /> Zahodiť označené
          </button>
          <button type="button" class="tl tiche" onclick={() => (vybrane = [])}>Zrušiť výber</button>
        </form>
      {/if}
    </div>
    {#each skupiny as sk (sk.id)}
      {#if sk.polozky.length > 1}
        <div class="skupina">
          <label class="oznacit">
            <input
              type="checkbox"
              checked={sk.polozky.every((z) => oznacene.includes(z.id))}
              onchange={(e) => prepniViac(sk.polozky.map((z) => z.id), e.currentTarget.checked)}
            />
            Skopírované naraz: {pocet(sk.polozky.length, 'odkaz', 'odkazy', 'odkazov')}
          </label>
          <span class="tlmene cisla">{cas(sk.polozky[0].cas)}</span>
        </div>
      {/if}
      <ul class="zoznam">
        {#each sk.polozky as z (z.id)}
          <li
            class="plocha polozka"
            class:oznacena={oznacene.includes(z.id)}
            class:nova={app.noveZoSchranky.includes(z.id)}
            data-zachyteny={z.id}
          >
            <input
              type="checkbox"
              class="volba"
              checked={oznacene.includes(z.id)}
              onchange={() => prepni(z.id)}
              aria-label="Označiť {z.nazov || z.url}"
            />
            <div class="nahlad">
              {#if z.nahlad}<img src={z.nahlad} alt="" loading="lazy" />{:else}<Ikona
                  meno={z.kolekcia ? 'playlist' : ikonaZdroja(z.zdroj)}
                  velkost={22}
                />{/if}
            </div>
            <div class="text">
              <h3 title={z.url}>{z.nazov || z.url}</h3>
              <p class="meta">
                <span class="stitok">{z.kolekcia ? `${z.zdroj} – playlist` : z.zdroj}</span>
                {#if z.ulozisko}<span>Súbor</span>{/if}
                {#if z.heslo}
                  <span
                    class="s-heslom"
                    title={z.heslo_zo_schranky ? 'Heslo našla Kometa v skopírovanom texte' : 'Heslo je zadané'}
                  >
                    <Ikona meno="zamok" velkost={13} />
                    {z.heslo_zo_schranky ? 'Heslo zo schránky' : 'S heslom'}
                  </span>
                {/if}
                <span class="tlmene cisla">{cas(z.cas)}</span>
                {#if z.nazov}<span class="tlmene odkaz vyber-text">{z.url}</span>{/if}
              </p>
              {#if upravuje === z.id}
                <form
                  class="heslo-riadok"
                  onsubmit={(e) => {
                    e.preventDefault()
                    ulozitHeslo([z.id], heslo)
                  }}
                >
                  <input
                    class="pole"
                    type="text"
                    autocomplete="off"
                    spellcheck="false"
                    bind:value={heslo}
                    placeholder="Heslo k súboru alebo archívu"
                    aria-label="Heslo k súboru alebo archívu"
                  />
                  <button class="tl hlavne" disabled={!heslo.trim()}>Uložiť</button>
                  {#if sk.polozky.length > 1}
                    <button
                      type="button"
                      class="tl"
                      disabled={!heslo.trim()}
                      onclick={() => ulozitHeslo(sk.polozky.map((x) => x.id), heslo)}
                    >
                      Celej skupine ({sk.polozky.length})
                    </button>
                  {/if}
                  {#if z.heslo}
                    <button type="button" class="tl tiche" onclick={() => ulozitHeslo([z.id], '')}>Odstrániť heslo</button>
                  {/if}
                </form>
              {/if}
            </div>
            <div class="tlacidla">
              {#if !z.kolekcia}
                <button
                  class="tl ikona"
                  class:zapnute={!!z.heslo}
                  title={z.heslo ? 'Zmeniť heslo' : 'Zadať heslo'}
                  aria-label={z.heslo ? 'Zmeniť heslo' : 'Zadať heslo'}
                  aria-expanded={upravuje === z.id}
                  onclick={() => upravit(z)}
                >
                  <Ikona meno="zamok" />
                </button>
              {/if}
              {#if z.kolekcia}
                <button class="tl" onclick={() => vybrat(z)}><Ikona meno="playlist" velkost={16} /> Vybrať videá</button>
              {:else}
                <button class="tl" onclick={() => stiahnut([z.id])}><Ikona meno="stiahnut" velkost={16} /> Stiahnuť</button>
              {/if}
              <button
                class="tl ikona nebezpecne"
                title="Zahodiť"
                aria-label="Zahodiť"
                onclick={() => skus('schranka_zahodit', [z.id])}
              >
                <Ikona meno="zavriet" />
              </button>
            </div>
          </li>
        {/each}
      </ul>
    {/each}
  {:else}
    <div class="prazdne">
      <Ikona meno="schranka" velkost={34} />
      <h2>Zatiaľ nič</h2>
      <p class="tlmene">
        {n.strazca
          ? 'Skopíruj odkaz (Ctrl+C) v prehliadači alebo inde – objaví sa tu.'
          : 'Zapni sledovanie schránky a skopíruj odkaz – objaví sa tu.'}
      </p>
    </div>
  {/if}
</div>

<style>
  .stranka {
    max-width: 1000px;
    padding: var(--s8) var(--s8) var(--s10);
  }
  header p {
    margin-top: var(--s1);
  }
  .nastavenie {
    display: grid;
    gap: var(--s4);
    margin-top: var(--s6);
    padding: var(--s5);
  }
  .rezim {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s5);
  }
  .rezim.zakazane {
    opacity: 0.5;
  }
  .rezim .vyber {
    max-width: 340px;
  }
  .nazov {
    font-weight: 600;
  }
  .lista {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    margin: var(--s8) 0 var(--s3);
  }
  .lista h2 {
    font-size: var(--t-l);
  }
  .akcie {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
  }
  .akcie .vyber {
    width: 260px;
  }
  .zoznam {
    display: grid;
    gap: var(--s2);
    list-style: none;
    padding: 0;
  }
  .rada {
    margin-bottom: var(--s3);
  }
  .vyber-riadok {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s3);
    margin-bottom: var(--s3);
  }
  .oznacit {
    display: inline-flex;
    align-items: center;
    gap: var(--s2);
    font-size: var(--t-s);
    cursor: pointer;
  }
  input[type='checkbox'] {
    width: 18px;
    height: 18px;
    accent-color: var(--accent);
    cursor: pointer;
  }
  .hromadne {
    position: sticky;
    top: var(--s2);
    z-index: 2;
    display: flex;
    flex: 1;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s2);
    padding: var(--s2) var(--s3);
    border: 1px solid var(--accent);
    border-radius: var(--r-ctl);
    background: var(--panel);
  }
  .hromadne .pole {
    width: 230px;
  }
  .pocet-oznacenych {
    font-weight: 600;
  }
  .skupina {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    margin: var(--s5) 0 var(--s2);
    padding: 0 var(--s1);
    font-weight: 600;
  }
  .zoznam + .zoznam {
    margin-top: var(--s2);
  }
  .polozka.nova {
    animation: nova 1.2s ease-out 3;
  }
  @keyframes nova {
    from {
      box-shadow: 0 0 0 3px var(--accent);
    }
    to {
      box-shadow: 0 0 0 3px transparent;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .polozka.nova {
      animation: none;
      box-shadow: 0 0 0 3px var(--accent);
    }
  }
  .polozka.oznacena {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .s-heslom {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--accent);
    font-weight: 600;
  }
  .heslo-riadok {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
    margin-top: var(--s2);
  }
  .heslo-riadok .pole {
    width: 280px;
  }
  .zapnute {
    color: var(--accent);
  }
  .polozka {
    display: grid;
    grid-template-columns: auto auto minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--s4);
    padding: var(--s3);
  }
  .nahlad {
    display: grid;
    place-items: center;
    width: 112px;
    aspect-ratio: 16 / 9;
    border-radius: var(--r-ctl);
    background: var(--sunken);
    color: var(--accent);
    overflow: hidden;
  }
  .nahlad img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .text h3 {
    overflow: hidden;
    font-size: var(--t-m);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s2);
    margin-top: var(--s1);
    font-size: var(--t-s);
  }
  .odkaz {
    overflow: hidden;
    max-width: 420px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tlacidla {
    display: flex;
    gap: var(--s1);
  }
  .prazdne {
    display: grid;
    justify-items: center;
    gap: var(--s2);
    margin-top: var(--s10);
    color: var(--muted);
    text-align: center;
  }
  .prazdne h2 {
    color: var(--ink);
  }
</style>
