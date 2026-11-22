<script lang="ts">
  import { volaj, ChybaJadra } from '../lib/api'
  import { app, skus, toast } from '../lib/app.svelte'
  import { pocet } from '../lib/format'
  import type { MenoIkony } from '../lib/ikony'
  import type { Nalez } from '../lib/typy'
  import Ikona from '../components/Ikona.svelte'
  import Kometa from '../components/Kometa.svelte'
  import Okno from '../components/Okno.svelte'

  let { url }: { url: string } = $props()

  // svelte-ignore state_referenced_locally
  let adresa = $state(url)
  let nalezy = $state<Nalez[] | null>(null)
  let vybrane = $state(new Set<number>())
  let nacitava = $state(false)
  let chyba = $state('')
  let heslo = $state('')
  let format = $state(app.nastavenia?.dl_preset ?? 'video')

  const IKONA: Record<string, MenoIkony> = {
    embed: 'film',
    stream: 'film',
    video: 'film',
    audio: 'hudba',
    subor: 'subor',
    titulky: 'jazyky',
  }

  async function prehladat() {
    if (!/^https?:\/\//.test(adresa.trim())) {
      chyba = 'Zadaj adresu stránky – začína na http:// alebo https://.'
      return
    }
    nacitava = true
    chyba = ''
    nalezy = null
    try {
      const n = await volaj<Nalez[]>('skenovat', adresa.trim())
      nalezy = n
      vybrane = new Set(n.flatMap((x, i) => (x.odporucane ? [i] : [])))
    } catch (e) {
      chyba = e instanceof ChybaJadra ? e.message : String(e)
    } finally {
      nacitava = false
    }
  }

  $effect(() => {
    if (url) prehladat()
  })

  function prepni(i: number) {
    const s = new Set(vybrane)
    if (s.has(i)) s.delete(i)
    else s.add(i)
    vybrane = s
  }

  async function pridat() {
    if (!nalezy) return
    const r = await skus<{ pridane: number }>('pridat_zo_skenu', [...vybrane].map((i) => nalezy![i]), format, heslo)
    if (!r) return
    toast(`Pridané do sťahovania: ${pocet(r.pridane, 'položka', 'položky', 'položiek')}.`, 'ok')
    app.dialog = null
    app.stranka = 'stahovanie'
  }
</script>

<Okno
  nazov="Prehľadať stránku"
  popis="Kometa nájde prehrávače, streamy a súbory aj vo vložených rámoch (napríklad Vimeo)."
  sirka={900}
  zavriet={() => (app.dialog = null)}
>
  <form
    class="adresa"
    onsubmit={(e) => {
      e.preventDefault()
      prehladat()
    }}
  >
    <label class="skryte" for="sk-url">Adresa stránky</label>
    <input id="sk-url" class="pole" bind:value={adresa} placeholder="https://…" />
    <button class="tl" disabled={nacitava}>{nacitava ? 'Hľadám…' : 'Prehľadať'}</button>
  </form>

  {#if nacitava}
    <div class="stav"><Kometa hodnota={null} popis="Prehľadávam stránku" /><p class="tlmene male">Prechádzam stránku a vložené prehrávače…</p></div>
  {:else if chyba}
    <p class="chyba vyber-text"><Ikona meno="pozor" velkost={18} /> {chyba}</p>
  {:else if nalezy && !nalezy.length}
    <p class="nic tlmene">
      Na stránke sa nenašlo nič na stiahnutie. Ak je obsah len po prihlásení, nastav prehliadač na stránke Účty.
    </p>
  {:else if nalezy}
    <ul class="zoznam">
      {#each nalezy as n, i (n.url)}
        {@const on = vybrane.has(i)}
        <li>
          <button class="nalez" class:on aria-pressed={on} disabled={!n.da_sa} onclick={() => prepni(i)}>
            <span class="zaskrt" aria-hidden="true">{#if on}<Ikona meno="fajka" velkost={14} hrubka={2.6} />{/if}</span>
            <span class="ikona"><Ikona meno={IKONA[n.druh] ?? 'subor'} velkost={18} /></span>
            <span class="text">
              <span class="riadok1">
                <strong>{n.nazov || n.poskytovatel}</strong>
                {#if n.odporucane}<span class="stitok akcent">Odporúčané</span>{/if}
              </span>
              <span class="riadok2 male tlmene">
                <span class="stitok">{n.druh_text}</span>
                <span>{n.poskytovatel}</span>
                <span class="url" title={n.url}>{n.url}</span>
              </span>
              {#if n.poznamka}<span class="male tlmene">{n.poznamka}</span>{/if}
            </span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  {#snippet pata()}
    <div class="heslo">
      <Ikona meno="zamok" velkost={17} />
      <label class="skryte" for="sk-heslo">Heslo k videu</label>
      <input id="sk-heslo" class="pole" type="password" bind:value={heslo} placeholder="Heslo k videu (ak ho má)" />
    </div>
    <span class="medzera"></span>
    <label class="skryte" for="sk-format">Formát</label>
    <select id="sk-format" class="vyber format" bind:value={format}>
      {#each app.init?.formaty ?? [] as f (f.id)}<option value={f.id}>{f.nazov}</option>{/each}
    </select>
    <button class="tl hlavne velke" disabled={!vybrane.size} onclick={pridat}>
      Stiahnuť {vybrane.size ? `(${vybrane.size})` : ''}
    </button>
  {/snippet}
</Okno>

<style>
  .adresa {
    display: flex;
    gap: var(--s2);
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
  .nic {
    padding: var(--s6) 0;
  }
  .zoznam {
    list-style: none;
    margin: var(--s4) 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .nalez {
    display: flex;
    align-items: center;
    gap: var(--s3);
    width: 100%;
    padding: var(--s2) var(--s3);
    border: 1px solid var(--line);
    border-radius: var(--r-ctl);
    background: var(--raised);
    color: var(--ink);
    text-align: left;
    cursor: pointer;
  }
  .nalez.on {
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }
  .nalez:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .zaskrt {
    display: grid;
    place-items: center;
    flex: none;
    width: 20px;
    height: 20px;
    border: 2px solid var(--line-strong);
    border-radius: 6px;
    color: var(--on-accent);
  }
  .on .zaskrt {
    border-color: var(--accent);
    background: var(--accent);
  }
  .ikona {
    color: var(--muted);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .riadok1 {
    display: flex;
    gap: var(--s2);
    align-items: center;
  }
  .riadok1 strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .riadok2 {
    display: flex;
    gap: var(--s3);
    align-items: center;
    min-width: 0;
  }
  .url {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .heslo {
    position: relative;
    width: 260px;
    color: var(--muted);
  }
  .heslo :global(svg) {
    position: absolute;
    left: 11px;
    top: 11px;
  }
  .heslo .pole {
    padding-left: 36px;
  }
  .medzera {
    flex: 1;
  }
  .format {
    width: 240px;
    height: 44px;
  }
</style>
