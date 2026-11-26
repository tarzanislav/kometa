<script lang="ts">
  // Kometa pre Python končí: pás s prechodom na aplikáciu (tá istá Kometa, bez Pythonu).
  import { app, skus } from '../lib/app.svelte'
  import Ikona from './Ikona.svelte'

  let pracuje = $state(false)
  let vysledok = $state<{ ok: boolean; text: string; stranka?: string } | null>(null)

  async function prejst() {
    pracuje = true
    vysledok = await skus('prechod_na_aplikaciu')
    pracuje = false
  }
</script>

{#if app.init?.koniec_pythonu}
  <div class="pas" role="status">
    <Ikona meno="balik" velkost={18} />
    <p>
      {#if vysledok}
        {vysledok.text}
        {#if vysledok.stranka && !vysledok.ok}
          <a href={vysledok.stranka} target="_blank" rel="noreferrer">Stránka vydaní</a>
        {/if}
      {:else}
        <strong>Kometa pre Python končí.</strong> Aplikácia Kometa vie všetko, čo táto, a viac – bez Pythonu,
        s jedným inštalátorom. Nastavenia, účty aj nedokončené sťahovania prevezme sama.
      {/if}
    </p>
    {#if !vysledok?.ok}
      <button class="tl hlavne" onclick={prejst} disabled={pracuje}>
        {pracuje ? 'Sťahujem inštalačku…' : 'Prejsť na aplikáciu'}
      </button>
    {/if}
  </div>
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
  }
</style>
