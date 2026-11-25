<script lang="ts">
  // Odkazy zachytené zo schránky (Ctrl+C) – stiahnuť, otvoriť výber z playlistu alebo zahodiť.
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

  async function stiahnut(ids: string[] | null) {
    const r = await skus<{ pridane: number; kolekcie: string[] }>('schranka_stiahnut', ids, format)
    if (!r) return
    if (r.pridane) toast(`Pridané do sťahovania: ${pocet(r.pridane, 'položka', 'položky', 'položiek')}.`, 'ok')
    if (r.kolekcie.length) app.dialog = { druh: 'playlist', url: r.kolekcie[0] }
  }

  function vybrat(z: Zachyteny) {
    skus('schranka_zahodit', [z.id])
    app.dialog = { druh: 'playlist', url: z.url }
  }

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
    <ul class="zoznam">
      {#each zoznam as z (z.id)}
        <li class="plocha polozka">
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
              <span class="tlmene cisla">{cas(z.cas)}</span>
              {#if z.nazov}<span class="tlmene odkaz vyber-text">{z.url}</span>{/if}
            </p>
          </div>
          <div class="tlacidla">
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
  .polozka {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
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
