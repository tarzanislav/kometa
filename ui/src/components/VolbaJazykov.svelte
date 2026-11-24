<script lang="ts">
  // Jazykové stopy pre viac videí naraz (playlist, kanál). Každé video dostane tie z jazykov,
  // ktoré má – v tomto poradí, so zvukom podľa voľby pri jazyku.
  import { BEZNE, KVALITY, ZVUKY, nazovJazyka, vlozJazyk, type VolbyJazykov, type Zvuk } from '../lib/jazyky'
  import { kratkyKod } from '../lib/format'
  import Ikona from './Ikona.svelte'
  import Prepinac from './Prepinac.svelte'
  import Segment from './Segment.svelte'

  let { volby = $bindable() }: { volby: VolbyJazykov } = $props()

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
    <h3 id="vj-nadpis">Jazykové stopy pre vybrané videá</h3>
    <p class="male tlmene">
      Každé video dostane tie z jazykov, ktoré má – v tomto poradí. Stopy idú po jazykoch: CZ (AAC), CZ (Opus), SK (AAC)…
    </p>
  </div>

  <ol class="jazyky" aria-label="Jazyky v poradí">
    {#each volby.poradie as kod, i (kod)}
      <li>
        <span class="cislo cisla" class:prvy={i === 0}>{i + 1}</span>
        <span class="meno" title={nazovJazyka(kod)}>{kratkyKod(kod)}</span>
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
