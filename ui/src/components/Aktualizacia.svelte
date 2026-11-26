<script lang="ts">
  // Pás o novej verzii Komety: nainštalovať hneď, alebo pri ďalšom spustení.
  import { app, skus, toast } from '../lib/app.svelte'
  import type { StavAktualizacie } from '../lib/typy'
  import Ikona from './Ikona.svelte'

  const a = $derived(app.stav?.aktualizacia)
  let skryta = $state(false)
  let pracuje = $state(false)

  const ukazat = $derived(
    !!a?.najnovsia && !skryta && (a.pripravena || a.instaluje || a.nainstalovana || a.balik === 'rucne'),
  )

  async function instalovat() {
    pracuje = true
    const r = await skus<StavAktualizacie>('aktualizacia_instaluj')
    pracuje = false
    if (r?.chyba) toast(r.chyba, 'chyba')
  }

  async function neskor() {
    await skus('aktualizacia_pri_starte')
    skryta = true
  }
</script>

{#if ukazat && a}
  <div class="pas" role="status">
    <Ikona meno="balik" velkost={18} />
    <p>
      {#if a.instaluje}
        Inštalujem Kometu {a.najnovsia} – okno sa na chvíľu zavrie a otvorí sa nová verzia.
      {:else if a.nainstalovana}
        Kometa {a.najnovsia} je nainštalovaná – platí od ďalšieho spustenia programu.
      {:else if a.balik === 'rucne'}
        Je dostupná nová verzia Komety {a.najnovsia}.
      {:else}
        Nová verzia Komety {a.najnovsia} je stiahnutá a overená.
        {a.pri_starte ? 'Ak ju nenainštaluješ teraz, nainštaluje sa sama pri ďalšom spustení.' : ''}
      {/if}
      {#if a.stranka}
        <a href={a.stranka} target="_blank" rel="noreferrer">Čo je nové</a>
      {/if}
    </p>
    {#if a.pripravena && !a.instaluje && !a.nainstalovana}
      <div class="akcie">
        <button class="tl hlavne" onclick={instalovat} disabled={pracuje}>Nainštalovať teraz</button>
        <button class="tl tiche" onclick={neskor}>{a.pri_starte ? 'Neskôr' : 'Pri ďalšom spustení'}</button>
      </div>
    {:else if !a.instaluje}
      <button class="tl tiche ikona" aria-label="Skryť" onclick={() => (skryta = true)}>
        <Ikona meno="zavriet" velkost={16} />
      </button>
    {/if}
  </div>
  {#if a.chyba}
    <p class="chyba male" role="alert">{a.chyba}</p>
  {/if}
{/if}

<style>
  .pas {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s3);
    align-items: center;
    margin: var(--s4) var(--s8) 0;
    padding: var(--s3) var(--s4);
    border-radius: var(--r-panel);
    background: var(--accent-soft);
    border: 1px solid var(--accent-line);
  }
  .pas p {
    flex: 1 1 320px;
    margin: 0;
  }
  .pas a {
    margin-left: var(--s2);
    white-space: nowrap;
  }
  .akcie {
    display: flex;
    gap: var(--s2);
  }
  .chyba {
    margin: var(--s2) var(--s8) 0;
    color: var(--err);
  }
</style>
