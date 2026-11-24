<script lang="ts">
  import { volaj, ChybaJadra } from '../lib/api'
  import { app, skus, toast } from '../lib/app.svelte'
  import { datum, dlzka, pocet } from '../lib/format'
  import type { Kolekcia } from '../lib/typy'
  import Ikona from '../components/Ikona.svelte'
  import Kometa from '../components/Kometa.svelte'
  import Okno from '../components/Okno.svelte'
  import Prepinac from '../components/Prepinac.svelte'
  import Segment from '../components/Segment.svelte'
  import VolbaJazykov from '../components/VolbaJazykov.svelte'
  import { PRIORITA, type VolbyJazykov } from '../lib/jazyky'

  let { url }: { url: string } = $props()

  // svelte-ignore state_referenced_locally
  let adresa = $state(url)
  let karta = $state<'videos' | 'shorts' | 'streams'>('videos')
  let data = $state<Kolekcia | null>(null)
  let nacitava = $state(false)
  let chyba = $state('')
  let hladat = $state('')
  let vybrane = $state(new Set<string>())
  let najnovsich = $state(10)
  let doPriecinka = $state(true)
  let format = $state(app.nastavenia?.dl_preset ?? 'video')
  let zobrazit = $state(240)
  // jazyky z nastavení (karta Jazykové stopy si ich pamätá)
  const n = app.nastavenia
  let jazyky = $state<VolbyJazykov>({
    poradie: [...(app.init?.jazyky_poradie?.length ? app.init.jazyky_poradie : PRIORITA)],
    original: n?.include_original ?? true,
    zvuk: n?.audio_mode ?? 'oboje',
    zvuky: {},
    kontajner: n?.kontajner ?? 'mkv',
    kvalita: n?.max_height || 'Najlepšia',
    rezim: 'video',
    bez_jazykov: 'povodny',
  })
  const sJazykmi = $derived(format === 'jazyky')

  const kanal = $derived(/youtube\.com\/(@|channel\/|c\/|user\/)/.test(adresa))
  const zoznam = $derived(
    (data?.entries ?? []).filter((e) => !hladat.trim() || e.title.toLowerCase().includes(hladat.trim().toLowerCase())),
  )
  const spolu = $derived((data?.entries ?? []).reduce((s, e) => s + (e.duration ?? 0), 0))

  async function nacitat() {
    if (!/^https?:\/\//.test(adresa.trim())) {
      chyba = 'Vlož odkaz na playlist alebo kanál.'
      return
    }
    nacitava = true
    chyba = ''
    data = null
    try {
      data = await volaj<Kolekcia>('kolekcia', adresa.trim(), karta)
      vybrane = new Set(data.entries.map((e) => e.url))
    } catch (e) {
      chyba = e instanceof ChybaJadra ? e.message : String(e)
    } finally {
      nacitava = false
    }
  }

  $effect(() => {
    if (url) nacitat()
  })

  function prepni(u: string) {
    const s = new Set(vybrane)
    if (s.has(u)) s.delete(u)
    else s.add(u)
    vybrane = s
  }

  async function pridat() {
    if (!data) return
    const polozky = data.entries.filter((e) => vybrane.has(e.url)).map((e) => ({ url: e.url, title: e.title }))
    const r = await skus<{ pridane: number; uz_vo_fronte: number }>(
      'pridat_vybrane',
      polozky,
      format,
      doPriecinka ? data.title : '',
      sJazykmi ? $state.snapshot(jazyky) : null,
    )
    if (!r) return
    toast(`Pridané do sťahovania: ${pocet(r.pridane, 'video', 'videá', 'videí')}.`, 'ok')
    app.dialog = null
    app.stranka = 'stahovanie'
  }
</script>

<Okno
  nazov={data?.title ?? 'Playlist alebo kanál'}
  popis={data
    ? [data.uploader && data.uploader !== data.title ? data.uploader : '', pocet(data.entries.length, 'video', 'videá', 'videí'), spolu ? `spolu ${dlzka(spolu)}` : '']
        .filter(Boolean)
        .join(', ')
    : 'Vyber, ktoré videá stiahnuť.'}
  sirka={980}
  zavriet={() => (app.dialog = null)}
>
  <form
    class="adresa"
    onsubmit={(e) => {
      e.preventDefault()
      nacitat()
    }}
  >
    <label class="skryte" for="pl-url">Odkaz na playlist alebo kanál</label>
    <input id="pl-url" class="pole" bind:value={adresa} placeholder="https://www.youtube.com/playlist?list=… alebo …/@kanal" />
    <button class="tl" disabled={nacitava}>{nacitava ? 'Načítavam…' : 'Načítať'}</button>
  </form>

  {#if kanal}
    <div class="karty">
      <Segment
        popis="Časť kanála"
        bind:hodnota={karta}
        zmena={() => nacitat()}
        moznosti={[
          { id: 'videos', text: 'Videá' },
          { id: 'shorts', text: 'Shorts' },
          { id: 'streams', text: 'Naživo' },
        ]}
      />
    </div>
  {/if}

  {#if nacitava}
    <div class="stav"><Kometa hodnota={null} popis="Načítavam zoznam" /><p class="tlmene male">Načítavam zoznam videí – pri veľkom kanáli to chvíľu trvá…</p></div>
  {:else if chyba}
    <p class="chyba vyber-text"><Ikona meno="pozor" velkost={18} /> {chyba}</p>
  {:else if data}
    <div class="nastroje">
      <div class="hladat">
        <Ikona meno="hladat" velkost={17} />
        <label class="skryte" for="pl-hladat">Hľadať v názvoch</label>
        <input id="pl-hladat" class="pole" bind:value={hladat} placeholder="Hľadať v názvoch" />
      </div>
      <button class="tl tiche" onclick={() => (vybrane = new Set(zoznam.map((e) => e.url)))}>Všetky</button>
      <button class="tl tiche" onclick={() => (vybrane = new Set())}>Žiadne</button>
      <span class="najnovsie">
        <button class="tl tiche" onclick={() => (vybrane = new Set(zoznam.slice(0, najnovsich).map((e) => e.url)))}>
          Najnovších
        </button>
        <input class="pole cisla" type="number" min="1" max="9999" bind:value={najnovsich} aria-label="Počet najnovších videí" />
      </span>
    </div>

    {#if sJazykmi}
      <VolbaJazykov bind:volby={jazyky} />
    {/if}

    <ul class="mriezka">
      {#each zoznam.slice(0, zobrazit) as e (e.url)}
        {@const on = vybrane.has(e.url)}
        <li>
          <button class="video" class:on aria-pressed={on} onclick={() => prepni(e.url)}>
            <span class="obr">
              {#if e.nahlad}<img src={e.nahlad} alt="" loading="lazy" referrerpolicy="no-referrer" />{/if}
              {#if e.duration}<span class="cas cisla">{dlzka(e.duration)}</span>{/if}
              <span class="znacka" aria-hidden="true">{#if on}<Ikona meno="fajka" velkost={14} hrubka={2.6} />{/if}</span>
            </span>
            <span class="nazov">{e.title}</span>
            {#if e.date}<span class="male tlmene">{datum(e.date)}</span>{/if}
          </button>
        </li>
      {/each}
    </ul>
    {#if zoznam.length > zobrazit}
      <button class="tl dalsie" onclick={() => (zobrazit += 240)}>Zobraziť ďalšie ({zoznam.length - zobrazit})</button>
    {/if}
  {/if}

  {#snippet pata()}
    <Prepinac nazov="Do vlastného priečinka" bind:zapnute={doPriecinka} popis={data ? `Priečinok „${data.title}“` : undefined} />
    <span class="medzera"></span>
    <label class="skryte" for="pl-format">Formát</label>
    <select id="pl-format" class="vyber format" bind:value={format}>
      {#each app.init?.formaty ?? [] as f (f.id)}
        <option value={f.id}>{f.id === 'jazyky' ? 'Video s jazykovými stopami' : f.nazov}</option>
      {/each}
    </select>
    <button
      class="tl hlavne velke"
      disabled={!data || !vybrane.size || (sJazykmi && !jazyky.poradie.length && !jazyky.original)}
      onclick={pridat}
    >
      Stiahnuť {vybrane.size ? pocet(vybrane.size, 'video', 'videá', 'videí') : ''}
    </button>
  {/snippet}
</Okno>

<style>
  .adresa {
    display: flex;
    gap: var(--s2);
  }
  .karty {
    margin-top: var(--s3);
  }
  .stav {
    display: flex;
    flex-direction: column;
    gap: var(--s2);
    padding: var(--s8) 0;
  }
  .chyba {
    display: flex;
    gap: var(--s2);
    padding: var(--s5) 0;
    color: var(--err);
    font-weight: 600;
  }
  .nastroje {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s1);
    align-items: center;
    margin: var(--s4) 0 var(--s3);
  }
  .hladat {
    position: relative;
    flex: 1;
    min-width: 220px;
    color: var(--muted);
  }
  .hladat :global(svg) {
    position: absolute;
    left: 11px;
    top: 10px;
  }
  .hladat .pole {
    padding-left: 36px;
  }
  .najnovsie {
    display: inline-flex;
    align-items: center;
  }
  .najnovsie .pole {
    width: 76px;
  }
  .mriezka {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(176px, 1fr));
    gap: var(--s3);
  }
  .video {
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 100%;
    padding: 0;
    border: 0;
    background: none;
    color: var(--ink);
    text-align: left;
    cursor: pointer;
  }
  .obr {
    position: relative;
    display: block;
    aspect-ratio: 16 / 9;
    overflow: hidden;
    border-radius: 10px;
    background: var(--sunken);
    outline: 2px solid transparent;
    outline-offset: 2px;
    transition: outline-color 0.15s, opacity 0.15s;
  }
  .video:not(.on) .obr {
    opacity: 0.55;
  }
  .video.on .obr {
    outline-color: var(--accent);
  }
  .obr img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .cas {
    position: absolute;
    right: 5px;
    bottom: 5px;
    padding: 1px 5px;
    border-radius: 5px;
    background: oklch(15% 0.02 var(--h) / 0.78);
    color: oklch(98% 0 0);
    font-size: 11px;
    font-weight: 700;
  }
  .znacka {
    position: absolute;
    left: 6px;
    top: 6px;
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 6px;
    border: 2px solid oklch(100% 0 0 / 0.9);
    background: oklch(15% 0.02 var(--h) / 0.35);
    color: var(--on-accent);
  }
  .on .znacka {
    border-color: var(--accent);
    background: var(--accent);
  }
  .nazov {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    font-size: var(--t-s);
    font-weight: 600;
    line-height: 1.35;
  }
  .dalsie {
    margin: var(--s4) auto 0;
    display: flex;
  }
  .medzera {
    flex: 1;
  }
  .format {
    width: 240px;
    height: 44px;
  }
</style>
