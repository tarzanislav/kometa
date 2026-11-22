<script lang="ts">
  import { volaj, ChybaJadra } from '../lib/api'
  import { app, nastav, skus, toast } from '../lib/app.svelte'
  import { dlzka, kratkyKod, pocet } from '../lib/format'
  import type { Stopa, VideoJazyky } from '../lib/typy'
  import Ikona from '../components/Ikona.svelte'
  import Kometa from '../components/Kometa.svelte'
  import Prepinac from '../components/Prepinac.svelte'
  import Segment from '../components/Segment.svelte'

  const n = app.nastavenia!
  let url = $state('')
  let nacitava = $state(false)
  let chybaNacitania = $state('')
  let video = $state<VideoJazyky | null>(null)
  let poradie = $state<string[]>([])
  let rezim = $state<'video' | 'existing' | 'audio'>(n.mode)
  let kontajner = $state<'mkv' | 'mp4'>(n.kontajner ?? 'mkv')
  let kvalita = $state(n.max_height || 'Najlepšia')
  let original = $state(n.include_original)
  let samostatne = $state(n.keep_separate)
  let subor = $state('')
  let zaznam = $state(false)
  let tahany = $state<number | null>(null)

  const kompat = $derived(app.nastavenia?.audio_mode !== 'opus')
  const uloha = $derived(app.stav?.jazyky)
  const stopy = $derived(video?.stopy ?? [])
  const podlaKodu = $derived(new Map(stopy.map((s) => [s.kod, s])))
  const vybrane = $derived(poradie.map((k) => podlaKodu.get(k)).filter((s): s is Stopa => !!s))
  const ostatne = $derived(stopy.filter((s) => !s.original && !poradie.includes(s.kod)))
  const orig = $derived(stopy.find((s) => s.original))
  const mozeStiahnut = $derived(
    !!video && (vybrane.length > 0 || (original && !!orig)) && !uloha?.busy && (rezim !== 'existing' || !!subor),
  )

  async function nacitat() {
    if (!url.trim() || nacitava) return
    nacitava = true
    chybaNacitania = ''
    video = null
    try {
      const v = await volaj<VideoJazyky>('jazyky_nacitat', url.trim())
      video = v
      poradie = v.vybrane
    } catch (e) {
      chybaNacitania = e instanceof ChybaJadra ? e.message : String(e)
    } finally {
      nacitava = false
    }
  }

  async function vlozit() {
    let text = ''
    try {
      text = await navigator.clipboard.readText()
    } catch {
      text = (await skus<string>('schranka')) ?? ''
    }
    const m = text.match(/https?:\/\/\S*(youtube\.com|youtu\.be)\S*/)
    if (m) {
      url = m[0]
      nacitat()
    } else toast('V schránke nie je odkaz na YouTube.', 'info')
  }

  function presun(z: number, na: number) {
    if (na < 0 || na >= poradie.length || z === na) return
    const p = [...poradie]
    const [x] = p.splice(z, 1)
    p.splice(na, 0, x)
    poradie = p
  }

  function predvolene() {
    if (!video) return
    poradie = video.vybrane
  }

  async function stiahnut() {
    if (!mozeStiahnut) return
    await skus('jazyky_stiahnut', {
      poradie,
      rezim,
      original,
      samostatne,
      kvalita,
      kontajner,
      zvuk: app.nastavenia?.audio_mode ?? 'oboje',
      subor,
    })
  }

  async function vybratSubor() {
    const s = await skus<string>('vybrat_video')
    if (s) subor = s
  }

  function zvukStopy(s: Stopa): { text: string; trieda: string; titul: string } {
    if (kontajner === 'mp4' || kompat) {
      return s.aac
        ? { text: 'AAC zo zdroja', trieda: 'ok', titul: 'YouTube ponúka AAC – stopa sa len prebalí, bez straty kvality.' }
        : {
            text: 'AAC prevodom',
            trieda: 'pozor',
            titul: 'YouTube ponúka len Opus – do AAC sa prekóduje (192 kb/s). Opus ostane v MKV navyše.',
          }
    }
    return { text: `Opus${s.kbps ? ` ${s.kbps} kb/s` : ''}`, trieda: '', titul: 'Pôvodný zvuk bez prekódovania.' }
  }
</script>

<div class="stranka">
  <header class="hlavicka">
    <h1>Jazykové stopy</h1>
    <p class="tlmene">
      Z videa na YouTube s preloženým zvukom stiahne vybrané jazyky a spojí ich s obrazom do jedného súboru. Jazyk
      prepneš v prehrávači.
    </p>
  </header>

  <form
    class="adresa plocha"
    onsubmit={(e) => {
      e.preventDefault()
      nacitat()
    }}
  >
    <label class="skryte" for="yt-url">Odkaz na video z YouTube</label>
    <Ikona meno="odkaz" velkost={20} />
    <input id="yt-url" class="pole" bind:value={url} placeholder="https://www.youtube.com/watch?v=…" spellcheck="false" />
    <button type="button" class="tl tiche" onclick={vlozit}><Ikona meno="schranka" velkost={17} /> Vložiť</button>
    <button class="tl" class:hlavne={!video} disabled={!url.trim() || nacitava}>
      {nacitava ? 'Načítavam…' : 'Načítať jazyky'}
    </button>
  </form>

  {#if nacitava}
    <div class="nacitavanie"><Kometa hodnota={null} popis="Načítavam video" /><p class="tlmene male">Zisťujem, aké jazyky video má…</p></div>
  {:else if chybaNacitania}
    <p class="hlaska chyba vyber-text"><Ikona meno="pozor" velkost={18} /> {chybaNacitania}</p>
  {/if}

  {#if video}
    <section class="video" aria-label="Video">
      {#if video.nahlad}<img src={video.nahlad} alt="" referrerpolicy="no-referrer" />{/if}
      <div>
        <h2>{video.nazov}</h2>
        <p class="tlmene male">
          {[video.autor, dlzka(video.dlzka), video.original && `pôvodne ${video.original.toLowerCase()}`].filter(Boolean).join(', ')}
        </p>
        <p class="male">{pocet(stopy.length, 'zvuková stopa', 'zvukové stopy', 'zvukových stôp')}</p>
      </div>
    </section>

    <div class="stlpce">
      <section class="plocha panel" aria-labelledby="h-poradie">
        <div class="panel-hlava">
          <h2 id="h-poradie">Jazyky a ich poradie</h2>
          <div class="rychle">
            <button class="tl tiche" onclick={predvolene}>Predvolené</button>
            <button class="tl tiche" onclick={() => (poradie = stopy.filter((s) => !s.original).map((s) => s.kod))}>Všetky</button>
            <button class="tl tiche" onclick={() => (poradie = [])}>Žiadne</button>
          </div>
        </div>
        <p class="tlmene male">Prvá stopa bude predvolená. Poradie zmeníš potiahnutím alebo šípkami.</p>

        {#if vybrane.length}
          <ol class="poradie">
            {#each vybrane as s, i (s.kod)}
              {@const z = zvukStopy(s)}
              <li
                draggable="true"
                class:tahany={tahany === i}
                ondragstart={(e) => {
                  tahany = i
                  e.dataTransfer?.setData('text/x-kometa-stopa', String(i))
                }}
                ondragend={() => (tahany = null)}
                ondragover={(e) => {
                  if (tahany == null) return
                  e.preventDefault()
                  e.stopPropagation()
                  if (tahany !== i) {
                    presun(tahany, i)
                    tahany = i
                  }
                }}
                ondrop={(e) => {
                  e.preventDefault()
                  e.stopPropagation()
                }}
              >
                <span class="uchop" aria-hidden="true"><Ikona meno="uchop" velkost={16} /></span>
                <span class="poradove cisla">{i + 1}</span>
                <span class="meno">{s.nazov}</span>
                <span class="kod">{kratkyKod(s.kod)}</span>
                <span class="stitok {z.trieda}" title={z.titul}>{z.text}</span>
                <span class="sipky">
                  <button class="tl ikona" aria-label="Posunúť {s.nazov} vyššie" disabled={i === 0} onclick={() => presun(i, i - 1)}>
                    <Ikona meno="hore" velkost={16} />
                  </button>
                  <button
                    class="tl ikona"
                    aria-label="Posunúť {s.nazov} nižšie"
                    disabled={i === vybrane.length - 1}
                    onclick={() => presun(i, i + 1)}
                  >
                    <Ikona meno="dole" velkost={16} />
                  </button>
                  <button
                    class="tl ikona nebezpecne"
                    aria-label="Odobrať {s.nazov}"
                    onclick={() => (poradie = poradie.filter((k) => k !== s.kod))}
                  >
                    <Ikona meno="zavriet" velkost={16} />
                  </button>
                </span>
              </li>
            {/each}
          </ol>
        {:else}
          <p class="prazdne-poradie tlmene">Nie je vybraný žiadny jazyk. Pridaj ho zo zoznamu nižšie.</p>
        {/if}

        {#if ostatne.length}
          <h3 class="dalsie">Ďalšie jazyky videa</h3>
          <div class="cipy">
            {#each ostatne as s (s.kod)}
              <button class="cip" onclick={() => (poradie = [...poradie, s.kod])}>
                <Ikona meno="plus" velkost={14} />
                {s.nazov}
              </button>
            {/each}
          </div>
        {/if}
      </section>

      <section class="plocha panel" aria-labelledby="h-vysledok">
        <h2 id="h-vysledok">Výsledok</h2>

        <Segment
          popis="Čo vytvoriť"
          bind:hodnota={rezim}
          moznosti={[
            { id: 'video', text: 'Video s jazykmi', ikona: 'film' },
            { id: 'existing', text: 'K môjmu videu', ikona: 'priecinok' },
            { id: 'audio', text: 'Len zvuk', ikona: 'hudba' },
          ]}
        />

        {#if rezim === 'video'}
          <div class="riadok">
            <label class="popis" for="kvalita">Kvalita obrazu</label>
            <select id="kvalita" class="vyber" bind:value={kvalita}>
              {#each ['Najlepšia', '2160p', '1440p', '1080p', '720p', '480p'] as q (q)}
                <option value={q}>{q === 'Najlepšia' ? 'Najlepšia dostupná' : `Najviac ${q}`}</option>
              {/each}
            </select>
          </div>
        {:else if rezim === 'existing'}
          <div class="riadok">
            <span class="popis">Tvoje stiahnuté video</span>
            <div class="subor">
              <span class="cesta vyber-text" class:tlmene={!subor} title={subor}>{subor || 'Nie je vybrané'}</span>
              <button class="tl" onclick={vybratSubor}>Vybrať…</button>
            </div>
          </div>
        {/if}

        <div class="riadok">
          <span class="popis">Formát súboru</span>
          <Segment
            popis="Formát súboru"
            bind:hodnota={kontajner}
            moznosti={[
              { id: 'mkv', text: rezim === 'audio' ? 'MKA' : 'MKV' },
              { id: 'mp4', text: rezim === 'audio' ? 'M4A' : 'MP4' },
            ]}
          />
          <p class="tlmene male vysvetlenie">
            {#if kontajner === 'mkv'}
              Udrží všetky stopy vrátane kvalitnejšieho Opus. Odporúčané pre počítač, VLC, Kodi a väčšinu televízorov.
            {:else}
              Obraz H.264 a zvuk AAC – prehrá ho takmer každé zariadenie. Stopy Opus sa nepridajú. H.264 dáva YouTube
              najviac v rozlíšení 1080p; ak treba viac, MP4 dostane VP9 alebo AV1.
            {/if}
          </p>
        </div>

        <div class="prepinace">
          <Prepinac
            nazov="Kompatibilný zvuk"
            zapnute={kompat || kontajner === 'mp4'}
            zakazane={kontajner === 'mp4'}
            popis={kontajner === 'mp4'
              ? 'MP4 má vždy zvuk AAC.'
              : 'Každý jazyk aj v AAC, ktoré prehrá každý prehrávač – tieto stopy sú prvé. Opus ostane za nimi.'}
            zmena={(v) => nastav({ audio_mode: v ? 'oboje' : 'opus' })}
          />
          {#if orig}
            <Prepinac
              nazov="Pôvodná stopa na koniec"
              bind:zapnute={original}
              popis="{orig.nazov} – jazyk, v ktorom bolo video natočené."
            />
          {/if}
          <Prepinac
            nazov="Každý jazyk aj samostatne"
            bind:zapnute={samostatne}
            popis="Popri spoločnom súbore uloží každú stopu ako vlastný zvukový súbor."
          />
        </div>

        {#if vybrane.length || (original && orig)}
          <div class="nahlad-poradia" aria-label="Poradie stôp vo výsledku">
            {#each vybrane as s, i (s.kod)}
              {#if i}<Ikona meno="vpravo" velkost={14} />{/if}
              <span class="kod-vysledok" class:prvy={i === 0}>{kratkyKod(s.kod)}</span>
            {/each}
            {#if original && orig}
              {#if vybrane.length}<Ikona meno="vpravo" velkost={14} />{/if}
              <span class="kod-vysledok">{kratkyKod(orig.kod)}</span>
            {/if}
            {#if kompat && kontajner === 'mkv'}<span class="tlmene male">a za nimi to isté v Opus</span>{/if}
          </div>
        {/if}

        <div class="spustit">
          <button class="tl hlavne velke" disabled={!mozeStiahnut} onclick={stiahnut}>
            <Ikona meno="stiahnut" velkost={18} hrubka={2} /> Stiahnuť s jazykmi
          </button>
          <span class="tlmene male cesta" title={app.nastavenia?.outdir}>do {app.nastavenia?.outdir}</span>
        </div>
      {#if uloha && (uloha.busy || uloha.file || uloha.error || uloha.log.length)}
        <div class="uloha" aria-live="polite" aria-label="Priebeh">
          {#if uloha.busy}
            <div class="uloha-hlava">
              <strong>{uloha.text || 'Pracujem…'}</strong>
              <span class="cisla tlmene">{Math.round(uloha.frac * 100)} %</span>
              <button class="tl" onclick={() => skus('jazyky_zrusit')}>Zrušiť</button>
            </div>
            <Kometa hodnota={uloha.frac} stav="bezi" hrubka={8} popis="Priebeh sťahovania jazykov" />
          {:else if uloha.file}
            <div class="uloha-hlava">
              <span class="hotovo"><Ikona meno="fajka" velkost={20} hrubka={2.2} /></span>
              <div class="hotovo-text">
                <strong>Hotovo</strong>
                <span class="male tlmene vyber-text" title={uloha.file}>{uloha.file.split(/[\\/]/).pop()}</span>
              </div>
              <button class="tl hlavne" onclick={() => skus('otvorit_subor', uloha!.file)}>
                <Ikona meno="priecinok" velkost={17} /> Zobraziť v priečinku
              </button>
            </div>
          {:else if uloha.error}
            <p class="hlaska chyba vyber-text"><Ikona meno="pozor" velkost={18} /> {uloha.error}</p>
          {:else if uloha.text}
            <p class="tlmene">{uloha.text}</p>
          {/if}
          {#if uloha.log.length}
            <button class="viac" onclick={() => (zaznam = !zaznam)} aria-expanded={zaznam}>
              {zaznam ? 'Skryť záznam' : 'Zobraziť záznam'}
            </button>
            {#if zaznam}<pre class="vyber-text">{uloha.log.join('\n')}</pre>{/if}
          {/if}
        </div>
      {/if}
      </section>
    </div>
  {/if}

</div>

<style>
  .stranka {
    max-width: 1180px;
    padding: var(--s8) var(--s8) var(--s10);
  }
  .hlavicka p {
    margin-top: var(--s1);
    max-width: 70ch;
  }
  .adresa {
    display: flex;
    align-items: center;
    gap: var(--s2);
    margin-top: var(--s5);
    padding: var(--s2) var(--s2) var(--s2) var(--s4);
    color: var(--accent-ink);
  }
  .adresa .pole {
    border-color: transparent;
    background: transparent;
    font-size: var(--t-l);
  }
  .adresa .pole:focus {
    box-shadow: none;
  }
  .adresa:focus-within {
    border-color: var(--accent-line);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .nacitavanie {
    display: flex;
    flex-direction: column;
    gap: var(--s2);
    margin-top: var(--s5);
    max-width: 420px;
  }
  .hlaska {
    display: flex;
    gap: var(--s2);
    align-items: flex-start;
    margin-top: var(--s4);
    font-weight: 600;
  }
  .hlaska.chyba {
    color: var(--err);
  }
  .video {
    display: flex;
    align-items: center;
    gap: var(--s5);
    margin: var(--s6) 0 var(--s5);
  }
  .video img {
    width: 176px;
    aspect-ratio: 16 / 9;
    object-fit: cover;
    border-radius: 12px;
    background: var(--sunken);
  }
  .video h2 {
    font-size: var(--t-xl);
    margin-bottom: var(--s1);
  }
  .stlpce {
    display: grid;
    grid-template-columns: minmax(0, 1.1fr) minmax(0, 1fr);
    gap: var(--s4);
    align-items: start;
  }
  .panel {
    padding: var(--s5);
    display: flex;
    flex-direction: column;
    gap: var(--s3);
  }
  .panel-hlava {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s2);
  }
  .rychle {
    display: flex;
  }
  .rychle .tl {
    height: 30px;
    padding: 0 var(--s2);
  }
  .poradie {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .poradie li {
    display: flex;
    align-items: center;
    gap: var(--s2);
    padding: 6px var(--s1) 6px var(--s2);
    border: 1px solid var(--line);
    border-radius: var(--r-ctl);
    background: var(--raised);
    cursor: grab;
    transition: border-color 0.15s, box-shadow 0.15s;
  }
  .poradie li:first-child {
    border-color: var(--accent-line);
  }
  .poradie li.tahany {
    opacity: 0.6;
    box-shadow: var(--shadow-pop);
  }
  .uchop {
    color: var(--faint);
  }
  .poradove {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: var(--sunken);
    color: var(--ink-2);
    font-size: var(--t-xs);
    font-weight: 700;
  }
  li:first-child .poradove {
    background: var(--accent);
    color: var(--on-accent);
  }
  .meno {
    flex: 1;
    min-width: 0;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .kod {
    color: var(--muted);
    font-size: var(--t-xs);
    font-weight: 700;
    letter-spacing: 0.02em;
  }
  .sipky {
    display: flex;
  }
  .sipky .tl {
    width: 28px;
    height: 28px;
  }
  .prazdne-poradie {
    padding: var(--s4);
    border: 1px dashed var(--line-strong);
    border-radius: var(--r-ctl);
    text-align: center;
  }
  .dalsie {
    margin-top: var(--s2);
    font-family: var(--font-text);
    font-size: var(--t-s);
    color: var(--ink-2);
  }
  .cipy {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .cip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 30px;
    padding: 0 var(--s3) 0 var(--s2);
    border: 1px dashed var(--line-strong);
    border-radius: 999px;
    background: transparent;
    color: var(--ink-2);
    font: 600 var(--t-s) / 1 var(--font-text);
    cursor: pointer;
  }
  .cip:hover {
    border-style: solid;
    border-color: var(--accent);
    color: var(--accent-ink);
    background: var(--accent-soft);
  }
  .riadok {
    display: flex;
    flex-direction: column;
    gap: var(--s1);
    margin-top: var(--s1);
  }
  .riadok .vyber {
    max-width: 260px;
  }
  .riadok .popis {
    display: block;
    font-size: var(--t-s);
    font-weight: 600;
    color: var(--ink-2);
  }
  .riadok :global(.segment) {
    align-self: flex-start;
  }
  .vysvetlenie {
    max-width: 60ch;
  }
  .subor {
    display: flex;
    gap: var(--s2);
    align-items: center;
  }
  .cesta {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .prepinace {
    display: flex;
    flex-direction: column;
    gap: var(--s3);
    padding: var(--s4) 0;
    border-top: 1px solid var(--line);
    border-bottom: 1px solid var(--line);
    margin-top: var(--s2);
  }
  .nahlad-poradia {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    color: var(--faint);
  }
  .kod-vysledok {
    padding: 3px 8px;
    border-radius: var(--r-tag);
    background: var(--sunken);
    color: var(--ink);
    font: 700 var(--t-s) / 1.2 var(--font-display);
  }
  .kod-vysledok.prvy {
    background: var(--accent);
    color: var(--on-accent);
  }
  .spustit {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: var(--s2);
    margin-top: var(--s2);
  }
  .uloha {
    margin-top: var(--s2);
    padding: var(--s4);
    border-radius: var(--r-ctl);
    background: var(--bg);
    display: flex;
    flex-direction: column;
    gap: var(--s3);
  }
  .uloha-hlava {
    display: flex;
    align-items: center;
    gap: var(--s3);
  }
  .uloha-hlava strong {
    flex: 1;
  }
  .hotovo {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background: var(--ok-soft);
    color: var(--ok);
  }
  .hotovo-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .hotovo-text span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .viac {
    align-self: flex-start;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent-ink);
    font: 600 var(--t-s) / 1.4 var(--font-text);
    cursor: pointer;
  }
  pre {
    margin: 0;
    padding: var(--s3);
    border-radius: var(--r-ctl);
    background: var(--sunken);
    color: var(--ink-2);
    font: 400 var(--t-xs) / 1.55 var(--font-text);
    white-space: pre-wrap;
    max-height: 220px;
    overflow: auto;
  }
  @media (max-width: 980px) {
    .stlpce {
      grid-template-columns: 1fr;
    }
  }
  @media (max-width: 860px) {
    .stranka {
      padding: var(--s5) var(--s4) var(--s8);
    }
  }
</style>
