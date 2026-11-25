<script lang="ts">
  // Kurzy po prihlásení: vlastné okno prehliadača → prihlásenie → načítať lekcie → stiahnuť
  // do priečinkov podľa kapitol.
  import { app, skus, toast } from '../lib/app.svelte'
  import { pocet } from '../lib/format'
  import type { Kurz } from '../lib/typy'
  import Ikona from '../components/Ikona.svelte'

  let adresa = $state('')
  let kurz = $state<Kurz | null>(null)
  let vybrane = $state<Set<string>>(new Set())
  let nacitava = $state(false)
  let format = $state('video')

  const prehliadac = $derived(app.stav?.prehliadac ?? null)
  const vsetkyLekcie = $derived(kurz?.kapitoly.flatMap((k) => k.lekcie) ?? [])
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
    <form
      class="adresa"
      onsubmit={(e) => {
        e.preventDefault()
        otvorit()
      }}
    >
      <label class="skryte" for="kurz-adresa">Adresa portálu alebo kurzu</label>
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
      <div class="kapitoly">
        {#each kurz.kapitoly as k, i (i)}
          {@const vsetky = k.lekcie.every((l) => vybrane.has(l.url))}
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
                    <span class="tlmene male adresa-lekcie">{l.url}</span>
                  </span>
                </li>
              {/each}
            </ul>
          </fieldset>
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
        Vybraté: {vybrane.size} z {vsetkyLekcie.length}. Každá lekcia dostane číslo podľa poradia na stránke, kapitoly
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
