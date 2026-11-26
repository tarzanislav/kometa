<script lang="ts">
  import { app, nastav, noc, skus } from '../lib/app.svelte'
  import { TEMY } from '../lib/temy'
  import Ikona from '../components/Ikona.svelte'
  import Prepinac from '../components/Prepinac.svelte'
  import type { StavAktualizacie } from '../lib/typy'
  import Segment from '../components/Segment.svelte'
  import NahladTemy from './NahladTemy.svelte'

  const n = $derived(app.nastavenia!)
  let rezim = $state(app.nastavenia?.rezim ?? 'auto')
  $effect(() => {
    rezim = app.nastavenia?.rezim ?? 'auto'
  })

  function cislo(kluc: 'parallel' | 'connections', v: number, min: number, max: number) {
    nastav({ [kluc]: Math.max(min, Math.min(max, v)) })
  }

  const nastroje = $derived(app.stav?.nastroje)
  const jadroRust = $derived(app.init?.jadro === 'rust')

  async function doinstalovat(co: 'ytdlp' | 'ffmpeg' | 'deno') {
    await skus(`doinstalovat_${co}`)
  }
  // priebeh a výsledok poslednej inštalácie patria k tomu pomocníkovi, ktorý sa inštaloval
  const instalujeFfmpeg = $derived(nastroje?.co === 'FFmpeg')

  async function zmenitPriecinok() {
    const p = await skus<string>('vybrat_priecinok')
    if (p && app.nastavenia) app.nastavenia.outdir = p
  }

  const akt = $derived(app.stav?.aktualizacia)
  const stavAktualizacie = $derived.by(() => {
    const a = akt
    if (!a) return ''
    if (a.stahuje) return 'Pozerám, či je nová verzia…'
    if (a.instaluje) return `Inštalujem Kometu ${a.najnovsia}…`
    if (a.nainstalovana) return `Kometa ${a.najnovsia} je nainštalovaná – platí od ďalšieho spustenia.`
    if (a.pripravena) return `Pripravená je Kometa ${a.najnovsia} (stiahnutá a overená).`
    if (a.najnovsia) return `Je dostupná Kometa ${a.najnovsia} – tu sa nainštaluje z GitHubu ručne.`
    if (a.chyba) return a.chyba
    return a.kontrola ? `Máš najnovšiu verziu ${a.verzia}.` : `Verzia ${a.verzia}.`
  })

  async function skontrolovat() {
    await skus<StavAktualizacie>('aktualizacia_skontroluj')
  }
</script>

<div class="stranka">
  <header>
    <h1>Nastavenia</h1>
  </header>

  <section aria-labelledby="h-vzhlad">
    <div class="sekcia-hlava">
      <h2 id="h-vzhlad">Vzhľad</h2>
      <Segment
        popis="Režim"
        bind:hodnota={rezim}
        zmena={(v) => nastav({ rezim: v })}
        moznosti={[
          { id: 'auto', text: 'Podľa systému', ikona: 'auto' },
          { id: 'den', text: 'Deň', ikona: 'slnko' },
          { id: 'noc', text: 'Noc', ikona: 'mesiac' },
        ]}
      />
    </div>
    <div class="temy" role="radiogroup" aria-label="Farebná téma">
      {#each TEMY as t (t.id)}
        <button
          class="tema"
          role="radio"
          aria-checked={n.tema === t.id}
          onclick={() => nastav({ tema: t.id })}
        >
          <NahladTemy tema={t.id} noc={noc()} />
          <span class="popis-temy">
            <span class="nazov-temy">{t.nazov}{#if n.tema === t.id}<Ikona meno="fajka" velkost={16} hrubka={2.2} />{/if}</span>
            <span class="male tlmene">{t.popis}</span>
          </span>
        </button>
      {/each}
    </div>
  </section>

  <section class="plocha blok" aria-labelledby="h-stahovanie">
    <h2 id="h-stahovanie">Sťahovanie</h2>
    <div class="riadky">
      <div class="nastavenie">
        <div>
          <span class="nazov">Súčasne sťahovať</span>
          <span class="male tlmene">Koľko súborov naraz.</span>
        </div>
        <div class="krokovac" role="group" aria-label="Súčasne sťahovať">
          <button class="tl ikona" aria-label="Menej" onclick={() => cislo('parallel', n.parallel - 1, 1, 6)} disabled={n.parallel <= 1}>
            <Ikona meno="minus" velkost={16} />
          </button>
          <span class="cisla">{n.parallel}</span>
          <button class="tl ikona" aria-label="Viac" onclick={() => cislo('parallel', n.parallel + 1, 1, 6)} disabled={n.parallel >= 6}>
            <Ikona meno="plus" velkost={16} />
          </button>
        </div>
      </div>
      <div class="nastavenie">
        <div>
          <label class="nazov" for="spojenia">Spojenia na jeden súbor</label>
          <span class="male tlmene">Viac spojení zrýchli veľké súbory a streamy. Ak server protestuje, Kometa ich sama ubere.</span>
        </div>
        <div class="posuvnik">
          <input
            id="spojenia"
            type="range"
            min="1"
            max="16"
            value={n.connections}
            onchange={(e) => cislo('connections', +(e.currentTarget as HTMLInputElement).value, 1, 16)}
            oninput={(e) => app.nastavenia && (app.nastavenia.connections = +(e.currentTarget as HTMLInputElement).value)}
          />
          <span class="cisla">{n.connections}</span>
        </div>
      </div>
      <div class="riadok-zvuk">
        <div>
          <p class="nazov-zvuk">Zvuk vo videách z YouTube</p>
          <p class="male tlmene">
            AAC prehrá každý prehrávač (ak ho YouTube nemá, prevedie sa z Opus), Opus je kvalitnejší. „AAC + Opus“ dá
            oboje do jedného MKV.
          </p>
        </div>
        <Segment
          popis="Zvuk vo videách z YouTube"
          hodnota={n.audio_mode ?? 'oboje'}
          zmena={(v) => nastav({ audio_mode: v })}
          moznosti={[
            { id: 'oboje', text: 'AAC + Opus' },
            { id: 'aac', text: 'Len AAC' },
            { id: 'opus', text: 'Len Opus' },
          ]}
        />
      </div>
      <Prepinac
        nazov="Po spustení pokračovať v nedokončených"
        zapnute={n.autoresume}
        popis="Rozpracované sťahovanie sa po zapnutí Komety samo rozbehne."
        zmena={(v) => nastav({ autoresume: v })}
      />
    </div>
  </section>

  <section class="plocha blok" aria-labelledby="h-vlozit">
    <h2 id="h-vlozit">Do stiahnutého videa vložiť</h2>
    <div class="riadky">
      <Prepinac
        nazov="Údaje o videu"
        zapnute={n.vlozit_udaje ?? true}
        popis="Názov, autor (kanál), dátum zverejnenia, odkaz na video a popis – ukážu ich prehrávače aj vlastnosti súboru."
        zmena={(v) => nastav({ vlozit_udaje: v })}
      />
      <Prepinac
        nazov="Kapitoly"
        zapnute={n.vlozit_kapitoly ?? true}
        popis="Ak ich video má (napr. na YouTube), v prehrávači preskočíš priamo na kapitolu."
        zmena={(v) => nastav({ vlozit_kapitoly: v })}
      />
      <Prepinac
        nazov="Obrázok videa"
        zapnute={n.vlozit_obal ?? true}
        popis="Náhľad videa ako obrázok súboru – uvidíš ho v priečinku aj v knižnici médií."
        zmena={(v) => nastav({ vlozit_obal: v })}
      />
    </div>
  </section>

  <section class="plocha blok" aria-labelledby="h-archivy">
    <h2 id="h-archivy">Archívy</h2>
    <div class="riadky">
      <Prepinac
        nazov="Rozbaliť archív po stiahnutí"
        zapnute={n.rozbalovat ?? true}
        popis="ZIP, 7z a RAR – aj zaheslované a rozdelené na časti. Heslo zadáš pri odkaze alebo pri položke."
        zmena={(v) => nastav({ rozbalovat: v })}
      />
      <div class="riadok-zvuk">
        <div>
          <p class="nazov-zvuk">Po rozbalení</p>
          <p class="male tlmene">Archív sa dá presunúť do Koša – odtiaľ sa dá obnoviť.</p>
        </div>
        <Segment
          popis="Čo s archívom po rozbalení"
          hodnota={n.po_rozbaleni ?? 'nechat'}
          zmena={(v) => nastav({ po_rozbaleni: v })}
          moznosti={[
            { id: 'nechat', text: 'Nechať archív' },
            { id: 'kos', text: 'Do Koša' },
          ]}
        />
      </div>
    </div>
  </section>

  <section class="plocha blok" aria-labelledby="h-priecinok">
    <h2 id="h-priecinok">Priečinok na sťahovanie</h2>
    <div class="priecinok">
      <Ikona meno="priecinok" velkost={20} />
      <span class="cesta vyber-text" title={n.outdir}>{n.outdir}</span>
      <button class="tl" onclick={zmenitPriecinok}>Zmeniť…</button>
      <button class="tl tiche" onclick={() => skus('otvorit')}>Otvoriť</button>
    </div>
  </section>

  {#if jadroRust}
    <section class="plocha blok" aria-labelledby="h-pomocnici">
      <h2 id="h-pomocnici">Pomocníci</h2>
      <div class="pomocnik">
        <div>
          <p class="pomocnik-nazov">yt-dlp</p>
          <p class="male tlmene">YouTube, Instagram, TikTok, Facebook, playlisty a stovky ďalších stránok. Kometa ho doinštaluje a aktualizuje sama{nastroje?.ytdlp_verzia ? ` (verzia ${nastroje.ytdlp_verzia})` : ''}.</p>
          {#if nastroje?.instaluje && nastroje.co === 'yt-dlp'}
            <div class="lista" role="progressbar" aria-label="Sťahovanie yt-dlp" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round((nastroje.podiel ?? 0) * 100)}>
              <span style:width="{Math.round((nastroje.podiel ?? 0) * 100)}%"></span>
            </div>
          {:else if nastroje?.text && nastroje.co === 'yt-dlp'}
            <p class="male tlmene">{nastroje.text}</p>
          {/if}
        </div>
        {#if nastroje?.ytdlp}
          <span class="stav-ok"><Ikona meno="fajka" velkost={16} /> Pripravený</span>
        {:else}
          <button class="tl" onclick={() => doinstalovat('ytdlp')} disabled={nastroje?.instaluje}>
            {nastroje?.instaluje && nastroje.co === 'yt-dlp' ? 'Sťahujem…' : 'Doinštalovať yt-dlp'}
          </button>
        {/if}
      </div>
      <div class="pomocnik">
        <div>
          <p class="pomocnik-nazov">Deno</p>
          <p class="male tlmene">JavaScript, ktorý yt-dlp potrebuje na YouTube. Kometa ho doinštaluje a aktualizuje sama.</p>
          {#if nastroje?.instaluje && nastroje.co === 'Deno'}
            <div class="lista" role="progressbar" aria-label="Sťahovanie Deno" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round((nastroje.podiel ?? 0) * 100)}>
              <span style:width="{Math.round((nastroje.podiel ?? 0) * 100)}%"></span>
            </div>
          {:else if nastroje?.text && nastroje.co === 'Deno'}
            <p class="male tlmene">{nastroje.text}</p>
          {/if}
        </div>
        {#if nastroje?.deno}
          <span class="stav-ok"><Ikona meno="fajka" velkost={16} /> Pripravené</span>
        {:else}
          <button class="tl" onclick={() => doinstalovat('deno')} disabled={nastroje?.instaluje}>
            {nastroje?.instaluje && nastroje.co === 'Deno' ? 'Sťahujem…' : 'Doinštalovať Deno'}
          </button>
        {/if}
      </div>
      <div class="pomocnik">
        <div>
          <p class="pomocnik-nazov">FFmpeg</p>
          <p class="male tlmene">Kometa sťahuje a spája obraz, zvuk aj titulky sama – aj zvuk Opus. FFmpeg treba už len na prevod do MP3 a na prevod Opus do AAC pri jazykových stopách.</p>
          {#if nastroje?.instaluje && instalujeFfmpeg}
            <div class="lista" role="progressbar" aria-label="Sťahovanie FFmpeg" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round((nastroje.podiel ?? 0) * 100)}>
              <span style:width="{Math.round((nastroje.podiel ?? 0) * 100)}%"></span>
            </div>
          {:else if nastroje?.text && instalujeFfmpeg}
            <p class="male tlmene">{nastroje.text}</p>
          {:else if !nastroje?.ffmpeg && !app.init?.funkcie?.doinstalovat_ffmpeg}
            <p class="male tlmene">V macOS ho nainštaluješ cez Homebrew: <code>brew install ffmpeg</code></p>
          {/if}
        </div>
        {#if nastroje?.ffmpeg}
          <span class="stav-ok"><Ikona meno="fajka" velkost={16} /> Nájdený</span>
        {:else if app.init?.funkcie?.doinstalovat_ffmpeg}
          <button class="tl" onclick={() => doinstalovat('ffmpeg')} disabled={nastroje?.instaluje}>
            {nastroje?.instaluje && instalujeFfmpeg ? 'Sťahujem…' : 'Doinštalovať FFmpeg'}
          </button>
        {:else}
          <span class="male tlmene">Nenájdený</span>
        {/if}
      </div>
    </section>
  {/if}

  <section class="plocha blok" aria-labelledby="h-aktualizacie">
    <h2 id="h-aktualizacie">Aktualizácie</h2>
    <div class="riadky">
      <Prepinac
        nazov="Aktualizovať automaticky"
        zapnute={n.aktualizacie ?? true}
        popis="Kometa sa občas pozrie na GitHub. Novú verziu stiahne, overí kontrolným súčtom a nainštaluje, keď potvrdíš – inak sama pri ďalšom spustení."
        zmena={(v) => nastav({ aktualizacie: v })}
      />
      <div class="riadok-zvuk">
        <p class="male tlmene" role="status">{stavAktualizacie}</p>
        <button class="tl" onclick={skontrolovat} disabled={akt?.stahuje || akt?.instaluje}>
          {akt?.stahuje ? 'Kontrolujem…' : 'Skontrolovať teraz'}
        </button>
      </div>
    </div>
  </section>

  <section class="o-programe" aria-label="O programe">
    <p class="male tlmene">
      Kometa {app.init?.verzia} – otvorený zdrojový kód pod licenciou MIT.
      <a href="https://github.com/tarzanislav/kometa" target="_blank" rel="noreferrer">github.com/tarzanislav/kometa</a>
    </p>
  </section>
</div>

<style>
  .stranka {
    max-width: 1000px;
    padding: var(--s8) var(--s8) var(--s10);
  }
  section {
    margin-top: var(--s6);
  }
  .sekcia-hlava {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s3);
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--s4);
  }
  .temy {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(196px, 1fr));
    gap: var(--s3);
  }
  .tema {
    display: flex;
    flex-direction: column;
    padding: 0;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--panel);
    color: var(--ink);
    text-align: left;
    cursor: pointer;
    transition: border-color 0.15s, box-shadow 0.15s, transform 0.15s;
  }
  .tema:hover {
    border-color: var(--line-strong);
  }
  .tema[aria-checked='true'] {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent);
  }
  .popis-temy {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: var(--s2) var(--s3) var(--s3);
  }
  .nazov-temy {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 700;
    color: var(--ink);
  }
  .nazov-temy :global(svg) {
    color: var(--accent-ink);
  }
  .blok {
    padding: var(--s5);
  }
  .blok h2 {
    margin-bottom: var(--s4);
  }
  .riadky {
    display: flex;
    flex-direction: column;
    gap: var(--s5);
  }
  .nastavenie {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s5);
  }
  .nastavenie > div:first-child {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-width: 62ch;
  }
  .nazov {
    font-weight: 600;
  }
  .krokovac {
    display: flex;
    align-items: center;
    gap: var(--s1);
    padding: 2px;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-ctl);
  }
  .krokovac span {
    min-width: 26px;
    text-align: center;
    font: 650 var(--t-l) / 1 var(--font-display);
  }
  .krokovac .tl {
    width: 30px;
    height: 30px;
  }
  .posuvnik {
    display: flex;
    align-items: center;
    gap: var(--s3);
  }
  .posuvnik span {
    min-width: 22px;
    font: 650 var(--t-l) / 1 var(--font-display);
  }
  input[type='range'] {
    width: 200px;
    accent-color: var(--accent);
  }
  .priecinok {
    display: flex;
    align-items: center;
    gap: var(--s3);
    color: var(--accent-ink);
  }
  .cesta {
    flex: 1;
    min-width: 0;
    color: var(--ink);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .o-programe {
    display: flex;
    flex-direction: column;
    gap: var(--s2);
  }
  .riadok-zvuk {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s3) var(--s5);
    align-items: center;
    justify-content: space-between;
    padding: var(--s3) 0;
  }
  .riadok-zvuk > div {
    max-width: 52ch;
  }
  .nazov-zvuk {
    font-weight: 650;
    margin-bottom: var(--s1);
  }
  .pomocnik {
    display: flex;
    gap: var(--s5);
    align-items: center;
    justify-content: space-between;
    padding: var(--s3) 0;
  }
  .pomocnik + .pomocnik {
    border-top: 1px solid var(--line);
  }
  .pomocnik > div {
    max-width: 60ch;
  }
  .pomocnik-nazov {
    font-weight: 650;
    margin-bottom: var(--s1);
  }
  .stav-ok {
    display: inline-flex;
    gap: var(--s2);
    align-items: center;
    color: var(--ok);
    font-weight: 600;
    white-space: nowrap;
  }
  .lista {
    height: 6px;
    margin-top: var(--s3);
    border-radius: 3px;
    background: var(--line);
    overflow: hidden;
  }
  .lista span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.3s ease;
  }
  @media (max-width: 860px) {
    .stranka {
      padding: var(--s5) var(--s4) var(--s8);
    }
    .nastavenie {
      flex-direction: column;
      align-items: flex-start;
    }
  }
</style>
