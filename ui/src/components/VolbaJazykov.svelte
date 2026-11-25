<script lang="ts">
  // Jazykové stopy pre viac videí naraz (playlist, kanál). Každé video dostane tie z jazykov,
  // ktoré má – v tomto poradí, so zvukom podľa voľby pri jazyku.
  import {
    BEZNE,
    KVALITY,
    ZVUKY,
    nazovJazyka,
    vlozJazyk,
    type VolbyJazykov,
    type ZisteneJazyky,
    type Zvuk,
  } from '../lib/jazyky'
  import { kratkyKod } from '../lib/format'
  import Ikona from './Ikona.svelte'
  import Kometa from './Kometa.svelte'
  import Prepinac from './Prepinac.svelte'
  import Segment from './Segment.svelte'
  import VolbaTitulkov from './VolbaTitulkov.svelte'

  let {
    volby = $bindable(),
    nadpis = 'Jazykové stopy pre vybrané videá',
    zistene = null,
    zistuje = 0,
    celkom = 0,
    zistit,
  }: {
    volby: VolbyJazykov
    nadpis?: string
    /** aké jazyky majú videá (vzorka alebo všetky) */
    zistene?: ZisteneJazyky | null
    /** koľko videí sa práve skúma (0 = nič) */
    zistuje?: number
    /** koľko videí je vybraných spolu */
    celkom?: number
    zistit?: () => void
  } = $props()

  const zakladny = (k: string) => k.toLowerCase().split('-')[0]
  const pocetJazyka = (kod: string) => zistene?.jazyky.find((j) => j.kod === zakladny(kod))?.pocet ?? 0
  /** nájdené jazyky, ktoré ešte nie sú v poradí */
  const dalsie = $derived(
    (zistene?.jazyky ?? []).filter((j) => !volby.poradie.some((k) => zakladny(k) === j.kod)),
  )
  const vzorkou = $derived(!!zistene && celkom > zistene.preskumanych)

  function pridajVsetkyNajdene() {
    let p = [...volby.poradie]
    for (const j of dalsie) p = vlozJazyk(p, j.kod)
    volby.poradie = p
  }

  let novy = $state('')
  const ponuka = $derived(BEZNE.filter(([k]) => !volby.poradie.includes(k)))
  const mp4 = $derived(volby.kontajner === 'mp4')
  const zvukJazyka = (kod: string): Zvuk => (mp4 ? 'aac' : (volby.zvuky[kod] ?? volby.zvuk))

  function presun(i: number, o: number) {
    const p = [...volby.poradie]
    const j = i + o
    if (j < 0 || j >= p.length) return
    ;[p[i], p[j]] = [p[j], p[i]]
    volby.poradie = p
  }

  function odober(kod: string) {
    volby.poradie = volby.poradie.filter((k) => k !== kod)
    const { [kod]: _, ...zvysok } = volby.zvuky
    volby.zvuky = zvysok
  }

  function pridaj(kod: string) {
    if (!kod) return
    volby.poradie = vlozJazyk(volby.poradie, kod)
    novy = ''
  }

  function zvukPreVsetky(z: Zvuk) {
    volby.zvuk = z
    volby.zvuky = {}
  }
</script>

<section class="volba" aria-labelledby="vj-nadpis">
  <div class="hlava">
    <h3 id="vj-nadpis">{nadpis}</h3>
    <p class="male tlmene">
      Každé video dostane tie z jazykov, ktoré má – v tomto poradí. Stopy idú po jazykoch: CZ (AAC), CZ (Opus), SK (AAC)…
    </p>
  </div>

  {#if zistit}
    <div class="zistene" aria-live="polite">
      {#if zistuje}
        <div class="zistuje">
          <Kometa hodnota={null} popis="Zisťujem jazyky" />
          <span class="male tlmene">Zisťujem, aké jazyky majú videá ({zistuje})…</span>
        </div>
      {:else if zistene}
        <p class="male">
          <strong>Jazyky vo videách</strong>
          <span class="tlmene">
            {vzorkou
              ? `– podľa ${zistene.preskumanych} z ${celkom} videí (vzorka z celého výberu)`
              : `– v ${zistene.preskumanych} ${zistene.preskumanych === 1 ? 'videe' : 'videách'}`}
          </span>
          <button class="tl tiche male-akcia" onclick={zistit}><Ikona meno="znova" velkost={14} /> Zistiť znova</button>
        </p>
        {#if dalsie.length}
          <div class="cipy">
            <span class="male tlmene">Pridať:</span>
            {#each dalsie as j (j.kod)}
              <button
                class="cip"
                title="{j.nazov} – v {j.pocet} z {zistene.preskumanych} videí{j.original ? ', niekde pôvodný jazyk' : ''}"
                onclick={() => (volby.poradie = vlozJazyk(volby.poradie, j.kod))}
              >
                + {kratkyKod(j.kod)} <span class="cisla tlmene">{j.pocet}/{zistene.preskumanych}</span>
              </button>
            {/each}
            {#if dalsie.length > 1}
              <button class="tl tiche male-akcia" onclick={pridajVsetkyNajdene}>Pridať všetky nájdené</button>
            {/if}
          </div>
        {:else if !zistene.jazyky.length}
          <p class="male tlmene">Vo videách sa nenašiel preložený zvuk – stiahnu sa s pôvodným zvukom (podľa voľby nižšie).</p>
        {/if}
      {:else}
        <button class="tl tiche" onclick={zistit}>
          <Ikona meno="hladat" velkost={15} /> Zistiť, aké jazyky videá majú
        </button>
      {/if}
    </div>
  {/if}

  <ol class="jazyky" aria-label="Jazyky v poradí">
    {#each volby.poradie as kod, i (kod)}
      <li>
        <span class="cislo cisla" class:prvy={i === 0}>{i + 1}</span>
        <span class="meno" title={nazovJazyka(kod)}>{kratkyKod(kod)}</span>
        {#if zistene && !zistuje}
          {@const p = pocetJazyka(kod)}
          <span
            class="vyskyt cisla"
            class:ziadny={p === 0}
            title={p ? `${nazovJazyka(kod)} je v ${p} z ${zistene.preskumanych} preskúmaných videí` : `${nazovJazyka(kod)} sa v preskúmaných videách nenašla`}
          >
            {p ? `${p}/${zistene.preskumanych}` : 'nie je'}
          </span>
        {/if}
        <select
          class="vyber maly"
          aria-label="Zvuk – {nazovJazyka(kod)}"
          value={zvukJazyka(kod)}
          disabled={mp4}
          onchange={(e) => (volby.zvuky = { ...volby.zvuky, [kod]: (e.currentTarget as HTMLSelectElement).value as Zvuk })}
        >
          {#each ZVUKY as z (z.id)}<option value={z.id}>{z.text}</option>{/each}
        </select>
        <span class="sipky">
          <button class="tl ikona male-tl" aria-label="Posunúť skôr" disabled={i === 0} onclick={() => presun(i, -1)}>
            <Ikona meno="hore" velkost={14} />
          </button>
          <button
            class="tl ikona male-tl"
            aria-label="Posunúť neskôr"
            disabled={i === volby.poradie.length - 1}
            onclick={() => presun(i, 1)}
          >
            <Ikona meno="dole" velkost={14} />
          </button>
          <button class="tl ikona male-tl" aria-label="Odobrať {nazovJazyka(kod)}" onclick={() => odober(kod)}>
            <Ikona meno="zavriet" velkost={14} />
          </button>
        </span>
      </li>
    {/each}
    <li class="pridat">
      <label class="skryte" for="vj-pridat">Pridať jazyk</label>
      <select id="vj-pridat" class="vyber maly" bind:value={novy} onchange={() => pridaj(novy)}>
        <option value="">+ Pridať jazyk…</option>
        {#each ponuka as [k, n] (k)}<option value={k}>{n}</option>{/each}
      </select>
    </li>
  </ol>

  <div class="mriezka-volieb">
    <div class="pole-volby">
      <span class="popis">Zvuk pre všetky jazyky</span>
      {#if mp4}
        <p class="male tlmene">MP4 má vždy zvuk AAC.</p>
      {:else}
        <Segment popis="Zvuk pre všetky jazyky" hodnota={volby.zvuk} zmena={zvukPreVsetky} moznosti={ZVUKY} />
      {/if}
    </div>
    <div class="pole-volby">
      <span class="popis">Keď video žiadny z jazykov nemá</span>
      <Segment
        popis="Keď video žiadny z jazykov nemá"
        bind:hodnota={volby.bez_jazykov}
        moznosti={[
          { id: 'povodny', text: 'Stiahnuť s pôvodným zvukom' },
          { id: 'preskocit', text: 'Preskočiť' },
        ]}
      />
    </div>
    <div class="pole-volby">
      <span class="popis">Výsledok</span>
      <div class="riadok-volieb">
        <Segment
          popis="Obsah"
          bind:hodnota={volby.rezim}
          moznosti={[
            { id: 'video', text: 'Video' },
            { id: 'audio', text: 'Len zvuk' },
          ]}
        />
        <Segment
          popis="Formát súboru"
          bind:hodnota={volby.kontajner}
          moznosti={[
            { id: 'mkv', text: 'MKV' },
            { id: 'mp4', text: 'MP4' },
          ]}
        />
        {#if volby.rezim === 'video'}
          <label class="skryte" for="vj-kvalita">Kvalita obrazu</label>
          <select id="vj-kvalita" class="vyber maly" bind:value={volby.kvalita}>
            {#each KVALITY as k (k)}<option value={k}>{k === 'Najlepšia' ? 'Najlepšia kvalita' : `najviac ${k}`}</option>{/each}
          </select>
        {/if}
      </div>
    </div>
    <div class="pole-volby">
      <VolbaTitulkov bind:titulky={volby.titulky} bind:auto={volby.titulky_auto} mp4={volby.kontajner === 'mp4'} />
    </div>
    <div class="pole-volby">
      <Prepinac
        nazov="Pôvodná stopa na koniec"
        bind:zapnute={volby.original}
        popis="Jazyk, v ktorom bolo video natočené – ak nie je medzi vybranými."
      />
    </div>
  </div>
</section>

<style>
  .volba {
    margin: var(--s3) 0 var(--s4);
    padding: var(--s4) var(--s5);
    border-radius: var(--r-panel);
    background: var(--bg);
    box-shadow: inset 0 0 0 1px var(--line);
  }
  .zistene {
    margin-top: var(--s3);
  }
  .zistene p {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s2);
  }
  .zistuje {
    display: flex;
    align-items: center;
    gap: var(--s3);
  }
  .zistuje :global(.kometa) {
    flex: 0 0 160px;
  }
  .cipy {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s1) var(--s2);
    margin-top: var(--s2);
  }
  .cip {
    padding: 3px 10px;
    border: 0;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent-ink, var(--ink));
    font: 600 var(--t-s) / 1.4 var(--font-text);
    cursor: pointer;
  }
  .cip:hover,
  .cip:focus-visible {
    background: var(--accent);
    color: var(--on-accent);
  }
  .male-akcia {
    height: 26px;
    padding: 0 var(--s2);
    font-size: var(--t-s);
  }
  .vyskyt {
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--ok-soft, var(--sunken));
    color: var(--ok, var(--ink-2));
    font-size: var(--t-xs);
    font-weight: 650;
  }
  .vyskyt.ziadny {
    background: var(--warn-soft, var(--sunken));
    color: var(--warn, var(--muted));
  }
  .hlava h3 {
    font: 650 var(--t-m) / 1.3 var(--font-display);
  }
  .hlava p {
    margin-top: var(--s1);
    max-width: 75ch;
  }
  .jazyky {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
    margin: var(--s3) 0;
    padding: 0;
    list-style: none;
  }
  .jazyky li {
    display: inline-flex;
    align-items: center;
    gap: var(--s2);
    padding: 4px 4px 4px 6px;
    border-radius: var(--r-ctl);
    background: var(--raised);
    box-shadow: inset 0 0 0 1px var(--line);
  }
  .jazyky li.pridat {
    padding: 0;
    background: none;
    box-shadow: none;
  }
  .cislo {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    font-size: var(--t-xs);
    font-weight: 700;
    background: var(--line);
  }
  .cislo.prvy {
    background: var(--accent);
    color: var(--on-accent);
  }
  .meno {
    font-weight: 700;
    letter-spacing: 0.02em;
  }
  .maly {
    width: auto;
    padding-block: 4px;
    font-size: var(--t-s);
  }
  .sipky {
    display: inline-flex;
  }
  .male-tl {
    width: 26px;
    height: 26px;
  }
  .mriezka-volieb {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: var(--s3) var(--s6);
  }
  .pole-volby {
    display: flex;
    flex-direction: column;
    gap: var(--s2);
  }
  .riadok-volieb {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
    align-items: center;
  }
  .popis {
    font-size: var(--t-s);
    font-weight: 600;
    color: var(--ink-2);
  }
</style>
