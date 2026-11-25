<script lang="ts">
  // Jazykové stopy pre viac videí naraz (vložené odkazy): zistí, aké jazyky má každé video,
  // a stiahne vybrané videá s rovnakými voľbami jazykov.
  import { volaj, ChybaJadra } from '../lib/api'
  import { app, skus, toast } from '../lib/app.svelte'
  import { dlzka, kratkyKod, pocet } from '../lib/format'
  import type { VolbyJazykov, ZisteneJazyky, ZisteneVideo } from '../lib/jazyky'
  import Ikona from '../components/Ikona.svelte'
  import VolbaJazykov from '../components/VolbaJazykov.svelte'

  let {
    odkazy,
    kolekcie = [],
    volby = $bindable(),
  }: { odkazy: string[]; kolekcie?: string[]; volby: VolbyJazykov } = $props()

  let zistene = $state<ZisteneJazyky | null>(null)
  let zistuje = $state(0)
  // svelte-ignore state_referenced_locally
  let vybrane = $state(new Set(odkazy))
  let priecinok = $state('')

  const videa = $derived<ZisteneVideo[]>(odkazy.map((u) => zistene?.videa.find((v) => v.url === u) ?? { url: u }))
  const zakladny = (k: string) => k.toLowerCase().split('-')[0]
  const zvolene = $derived(new Set(volby.poradie.map(zakladny)))

  async function zistit() {
    if (zistuje) return
    zistuje = odkazy.length
    try {
      zistene = await volaj<ZisteneJazyky>('jazyky_zistit', odkazy)
      // videá, ktoré sa nedajú načítať, sa nevyberú
      vybrane = new Set(odkazy.filter((u) => !zistene?.videa.find((v) => v.url === u)?.chyba))
    } catch (e) {
      toast(e instanceof ChybaJadra ? e.message : String(e), 'chyba')
    } finally {
      zistuje = 0
    }
  }

  $effect(() => {
    if (odkazy.length && !zistene && !zistuje) zistit()
  })

  function prepni(u: string) {
    const s = new Set(vybrane)
    if (s.has(u)) s.delete(u)
    else s.add(u)
    vybrane = s
  }

  async function stiahnut() {
    const polozky = videa.filter((v) => vybrane.has(v.url)).map((v) => ({ url: v.url, title: v.nazov ?? '' }))
    const r = await skus<{ pridane: number; uz_vo_fronte: number }>(
      'pridat_vybrane',
      polozky,
      'jazyky',
      priecinok.trim(),
      $state.snapshot(volby),
    )
    if (!r) return
    toast(`Do sťahovania: ${pocet(r.pridane, 'video', 'videá', 'videí')} s jazykovými stopami.`, 'ok')
    app.stranka = 'stahovanie'
  }
</script>

{#if kolekcie.length}
  <div class="plocha kolekcie">
    <Ikona meno="playlist" velkost={20} />
    <p>
      {kolekcie.length === 1 ? 'Medzi odkazmi je playlist alebo kanál' : `Medzi odkazmi je ${kolekcie.length}× playlist alebo kanál`}
      – videá z neho vyberieš zvlášť, s rovnakými jazykmi.
    </p>
    {#each kolekcie as k (k)}
      <button class="tl" onclick={() => (app.dialog = { druh: 'playlist', url: k, jazyky: $state.snapshot(volby) })}>
        Vybrať videá
      </button>
    {/each}
  </div>
{/if}

{#if odkazy.length}
  <VolbaJazykov
    bind:volby
    nadpis={`Jazykové stopy pre ${pocet(odkazy.length, 'video', 'videá', 'videí')}`}
    {zistene}
    {zistuje}
    celkom={odkazy.length}
    {zistit}
  />

  <ul class="zoznam">
    {#each videa as v (v.url)}
      {@const on = vybrane.has(v.url)}
      <li class="plocha video" class:vypnute={!on}>
        <input type="checkbox" checked={on} onchange={() => prepni(v.url)} aria-label="Stiahnuť {v.nazov || v.url}" />
        <span class="obr">{#if v.nahlad}<img src={v.nahlad} alt="" loading="lazy" referrerpolicy="no-referrer" />{/if}</span>
        <span class="text">
          <span class="nazov" title={v.url}>{v.nazov || v.url}</span>
          {#if v.chyba}
            <span class="male chyba">{v.chyba}</span>
          {:else if v.jazyky}
            <span class="jazyky male">
              {#if v.dlzka}<span class="tlmene cisla">{dlzka(v.dlzka)}</span>{/if}
              {#each v.jazyky as k (k)}
                <span class="jazyk" class:zvoleny={zvolene.has(k)} class:orig={v.original === k} title={v.original === k ? 'pôvodný jazyk' : ''}>
                  {kratkyKod(k)}
                </span>
              {/each}
              {#if !v.jazyky.some((k) => zvolene.has(k))}
                <span class="tlmene">– žiadny z vybraných jazykov</span>
              {/if}
            </span>
          {:else}
            <span class="male tlmene">Zisťujem jazyky…</span>
          {/if}
        </span>
      </li>
    {/each}
  </ul>

  <div class="pata plocha">
    <label class="popis" for="jv-priecinok">Do priečinka <span class="tlmene">(nepovinné)</span></label>
    <input id="jv-priecinok" class="pole" bind:value={priecinok} placeholder="napr. Dokumenty o histórii" />
    <span class="medzera"></span>
    <button
      class="tl hlavne velke"
      disabled={!vybrane.size || (!volby.poradie.length && !volby.original)}
      onclick={stiahnut}
    >
      <Ikona meno="stiahnut" velkost={18} hrubka={2} /> Stiahnuť {pocet(vybrane.size, 'video', 'videá', 'videí')}
    </button>
  </div>
{/if}

<style>
  .kolekcie {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s3);
    margin-top: var(--s4);
    padding: var(--s3) var(--s4);
  }
  .kolekcie p {
    flex: 1;
    min-width: 260px;
  }
  .zoznam {
    display: grid;
    gap: var(--s2);
    padding: 0;
    list-style: none;
  }
  .video {
    display: grid;
    grid-template-columns: auto auto minmax(0, 1fr);
    align-items: center;
    gap: var(--s3);
    padding: var(--s2) var(--s3);
  }
  .video.vypnute {
    opacity: 0.55;
  }
  .obr {
    width: 96px;
    aspect-ratio: 16 / 9;
    overflow: hidden;
    border-radius: var(--r-ctl);
    background: var(--sunken);
  }
  .obr img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .text {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .nazov {
    overflow: hidden;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .jazyky {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
  }
  .jazyk {
    padding: 0 6px;
    border-radius: 999px;
    background: var(--sunken);
    color: var(--ink-2);
    font-weight: 650;
    font-size: var(--t-xs);
    line-height: 1.7;
  }
  .jazyk.zvoleny {
    background: var(--accent-soft);
    color: var(--accent-ink);
  }
  .jazyk.orig {
    box-shadow: inset 0 0 0 1px var(--line);
  }
  .pata {
    position: sticky;
    bottom: var(--s3);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s3);
    margin-top: var(--s4);
    padding: var(--s3) var(--s4);
    box-shadow: var(--shadow-pop);
  }
  .pata .pole {
    width: 280px;
  }
  .medzera {
    flex: 1;
  }
</style>
