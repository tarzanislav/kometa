<script lang="ts">
  // Funkcia, ktorú toto jadro ešte nemá (aplikácia v Ruste) – povie, čo robiť dnes.
  import Prazdne from '../components/Prazdne.svelte'

  let { nadpis, text }: { nadpis: string; text: string } = $props()
</script>

<div class="stranka">
  <header><h1>{nadpis}</h1></header>
  <div class="pribudne">
    <Prazdne />
    <h2>Pribudne v ďalšej verzii aplikácie</h2>
    <p class="tlmene">{text}</p>
  </div>
</div>

<style>
  .stranka {
    max-width: 1000px;
    padding: var(--s8) var(--s8) var(--s10);
  }
  .pribudne {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--s3);
    margin-top: var(--s10);
    text-align: center;
  }
  .pribudne :global(svg) {
    width: min(440px, 100%);
  }
  .pribudne p {
    max-width: 52ch;
  }
</style>
