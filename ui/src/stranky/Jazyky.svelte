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
  /** zvuk pre všetky jazyky a voľba pri jednotlivých jazykoch (prebije spoločnú) */
  let zvukVsetky = $state<Zvuk>(n.audio_mode ?? 'oboje')
  let zvuky = $state<Record<string, Zvuk>>({})

  type Zvuk = 'oboje' | 'aac' | 'opus'
  const ZVUKY: { id: Zvuk; text: string }[] = [
    { id: 'oboje', text: 'AAC + Opus' },
    { id: 'aac', text: 'Len AAC' },
    { id: 'opus', text: 'Len Opus' },
  ]
  /** Poradie jazykov: CZ, SK, EN, PL, HU, RU – ostatné za nimi. */
  const PRIORITA = ['cs', 'sk', 'en', 'pl', 'hu', 'ru']
  const priorita = (kod: string) => {
    const i = PRIORITA.indexOf(kod.toLowerCase().split('-')[0])
    return i < 0 ? Infinity : i
  }
  const uloha = $derived(app.stav?.jazyky)
  const stopy = $derived(video?.stopy ?? [])
  const podlaKodu = $derived(new Map(stopy.map((s) => [s.kod, s])))
  const vybrane = $derived(poradie.map((k) => podlaKodu.get(k)).filter((s): s is Stopa => !!s))
  const ostatne = $derived(stopy.filter((s) => !s.original && !poradie.includes(s.kod)))
  const orig = $derived(stopy.find((s) => s.original))
  const zvukStopy = (kod: string): Zvuk => (kontajner === 'mp4' ? 'aac' : (zvuky[kod] ?? zvukVsetky))
  /** Stopy výsledku v poradí CZ (AAC), CZ (Opus), SK (AAC)… */
  const vysledok = $derived.by(() => {
    const jazyky = [...vybrane]
    if (original && orig && !jazyky.some((s) => s.kod === orig.kod)) jazyky.push(orig)
    return jazyky.flatMap((s) => {
      const z = zvukStopy(s.kod)
      const kody: ('AAC' | 'Opus')[] = z === 'oboje' ? ['AAC', 'Opus'] : [z === 'aac' ? 'AAC' : 'Opus']
      return kody.map((k) => ({ kod: s.kod, kodek: k }))
    })
  })
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
      zvuky = {}
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

  /** Pridá jazyk: z poradia CZ, SK, EN, PL, HU, RU na svoje miesto, ostatné na koniec. */
  function pridat(kod: string) {
    const p = priorita(kod)
    const j = p === Infinity ? -1 : poradie.findIndex((k) => priorita(k) > p)
    poradie = j < 0 ? [...poradie, kod] : [...poradie.slice(0, j), kod, ...poradie.slice(j)]
  }

  function vsetky() {
    const kody = stopy.filter((s) => !s.original).map((s) => s.kod)
    poradie = kody.map((k, i) => [k, i] as const).sort((a, b) => priorita(a[0]) - priorita(b[0]) || a[1] - b[1]).map(([k]) => k)
  }

  function zvukPreVsetky(z: Zvuk) {
    zvukVsetky = z
    zvuky = {}
    nastav({ audio_mode: z })
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
      zvuk: zvukVsetky,
      zvuky: Object.fromEntries(Object.entries(zvuky).filter(([, z]) => z !== zvukVsetky)),
      subor,
    })
  }

  async function vybratSubor() {
    const s = await skus<string>('vybrat_video')
    if (s) subor = s
  }

  /** Čo z jazyka vznikne – podľa voľby a toho, čo YouTube ponúka. */
  function popisZvuku(s: Stopa): { text: string; trieda: string; titul: string } {
    const z = zvukStopy(s.kod)
    const opus = `Opus${s.kbps ? ` ${s.kbps} kb/s` : ''}`
    if (z === 'opus') return { text: opus, trieda: '', titul: 'Pôvodný zvuk Opus bez prekódovania.' }
    const aac = s.aac
      ? { text: 'AAC zo zdroja', trieda: 'ok', titul: 'YouTube ponúka AAC – stopa sa len prebalí, bez straty kvality.' }
      : {
          text: 'AAC prevodom',
          trieda: 'pozor',
          titul: 'YouTube ponúka len Opus – do AAC sa prekóduje (192 kb/s, treba FFmpeg).',
        }
    if (z === 'aac') return aac
    return { text: `${aac.text} + Opus`, trieda: aac.trieda, titul: `${aac.titul} K tomu pôvodný Opus.` }
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
            <button class="tl tiche" onclick={vsetky}>Všetky</button>
            <button class="tl tiche" onclick={() => (poradie = [])}>Žiadne</button>
          </div>
        </div>
        <p class="tlmene male">
          Prvá stopa bude predvolená. Poradie CZ, SK, EN, PL, HU, RU – ďalšie jazyky idú za nimi. Zmeníš ho potiahnutím
          alebo šípkami.
        </p>

        {#if vybrane.length}
          <ol class="poradie">
            {#each vybrane as s, i (s.kod)}
              {@const z = popisZvuku(s)}
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
                <span class="meno-blok">
                  <span class="meno">{s.nazov}</span>
                  <span class="kod">{kratkyKod(s.kod)}</span>
                </span>
                <span class="zvuk-blok">
                <select
                  class="vyber zvuk-stopy"
                  aria-label="Zvuk – {s.nazov}"
                  value={zvukStopy(s.kod)}
                  disabled={kontajner === 'mp4'}
                  onchange={(e) => (zvuky = { ...zvuky, [s.kod]: (e.currentTarget as HTMLSelectElement).value as Zvuk })}
                >
                  {#each ZVUKY as m (m.id)}<option value={m.id}>{m.text}</option>{/each}
                </select>
                <span class="stitok {z.trieda}" title={z.titul}>{z.text}</span>
                </span>
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
              <button class="cip" onclick={() => pridat(s.kod)}>
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

        <div class="riadok">
          <span class="popis">Zvuk pre všetky jazyky</span>
          {#if kontajner === 'mp4'}
            <p class="tlmene male vysvetlenie">MP4 má vždy zvuk AAC – ak ho YouTube neponúka, prevedie sa z Opus.</p>
          {:else}
            <Segment popis="Zvuk pre všetky jazyky" hodnota={zvukVsetky} zmena={zvukPreVsetky} moznosti={ZVUKY} />
            <p class="tlmene male vysvetlenie">
              AAC prehrá každý prehrávač; ak ho YouTube nemá, prevedie sa z Opus. Opus je kvalitnejší. Pri každom jazyku
              to môžeš zmeniť zvlášť.
            </p>
          {/if}
        </div>

        <div class="prepinace">
          {#if orig}
            <Prepinac
              nazov="Pôvodná stopa na koniec"
              bind:zapnute={original}
              popis="{orig.nazov} – jazyk, v ktorom bolo video natočené."
            />
            {#if original && kontajner !== 'mp4' && !poradie.includes(orig.kod)}
              <div class="zvuk-originalu">
                <span class="male tlmene">Zvuk originálu</span>
                <select
                  class="vyber zvuk-stopy"
                  aria-label="Zvuk – {orig.nazov} (originál)"
                  value={zvukStopy(orig.kod)}
                  onchange={(e) => (zvuky = { ...zvuky, [orig!.kod]: (e.currentTarget as HTMLSelectElement).value as Zvuk })}
                >
                  {#each ZVUKY as m (m.id)}<option value={m.id}>{m.text}</option>{/each}
                </select>
              </div>
            {/if}
          {/if}
          <Prepinac
            nazov="Každý jazyk aj samostatne"
            bind:zapnute={samostatne}
            popis="Popri spoločnom súbore uloží každú stopu ako vlastný zvukový súbor."
          />
        </div>

        {#if vysledok.length}
          <div class="nahlad-poradia" aria-label="Poradie zvukových stôp vo výsledku">
            {#each vysledok as st, i (`${st.kod}-${st.kodek}`)}
              {#if i}<Ikona meno="vpravo" velkost={14} />{/if}
              <span class="kod-vysledok" class:prvy={i === 0} class:opus={st.kodek === 'Opus'}>
                {kratkyKod(st.kod)} <small>{st.kodek}</small>
              </span>
            {/each}
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
    /* riadok 1: úchop, poradie, jazyk, šípky – riadok 2: zvuk (výber a čo z neho vznikne) */
    display: grid;
    grid-template-columns: auto auto minmax(0, 1fr) auto;
    grid-template-areas:
      'uchop cislo meno sipky'
      '. . zvuk zvuk';
    align-items: center;
    gap: 4px var(--s2);
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
  .uchop {
    grid-area: uchop;
  }
  .poradove {
    grid-area: cislo;
  }
  .meno-blok {
    grid-area: meno;
    display: flex;
    align-items: baseline;
    gap: var(--s2);
    min-width: 0;
  }
  .zvuk-blok {
    grid-area: zvuk;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s2);
  }
  .meno {
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
    grid-area: sipky;
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
  .kod-vysledok small {
    font: 600 var(--t-xs) / 1 var(--font-body);
    opacity: 0.8;
  }
  .kod-vysledok.opus:not(.prvy) {
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--line);
  }
  .zvuk-stopy {
    width: auto;
    min-width: 0;
    padding-block: 4px;
    font-size: var(--t-s);
  }
  .zvuk-originalu {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s3);
    margin: calc(-1 * var(--s1)) 0 var(--s1);
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
