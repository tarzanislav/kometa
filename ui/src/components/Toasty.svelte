<script lang="ts">
  import { app, zavrietToast } from '../lib/app.svelte'
  import Ikona from './Ikona.svelte'
</script>

<div class="toasty" aria-live="polite">
  {#each app.toasty as t (t.id)}
    <div class="toast {t.druh}" role={t.druh === 'chyba' ? 'alert' : 'status'}>
      <Ikona meno={t.druh === 'chyba' ? 'pozor' : t.druh === 'ok' ? 'fajka' : 'info'} velkost={18} />
      <p class="vyber-text">{t.text}</p>
      {#if t.akcia}
        <button
          class="tl tiche"
          onclick={() => {
            t.akcia?.spusti()
            zavrietToast(t.id)
          }}>{t.akcia.text}</button
        >
      {/if}
      <button class="tl ikona" aria-label="Zavrieť upozornenie" onclick={() => zavrietToast(t.id)}>
        <Ikona meno="zavriet" velkost={16} />
      </button>
    </div>
  {/each}
</div>

<style>
  .toasty {
    position: fixed;
    right: var(--s5);
    bottom: var(--s5);
    z-index: 50;
    display: flex;
    flex-direction: column;
    gap: var(--s2);
    width: min(420px, calc(100vw - 40px));
  }
  .toast {
    display: flex;
    align-items: center;
    gap: var(--s3);
    padding: var(--s2) var(--s2) var(--s2) var(--s4);
    border: 1px solid var(--line);
    border-radius: var(--r-panel);
    background: var(--raised);
    box-shadow: var(--shadow-pop);
    animation: prilet 0.25s var(--ease);
  }
  .toast p {
    flex: 1;
    min-width: 0;
    font-size: var(--t-s);
  }
  .ok {
    color: var(--ok);
  }
  .chyba {
    color: var(--err);
  }
  .info {
    color: var(--accent-ink);
  }
  .toast p {
    color: var(--ink);
  }
  .toast .tl {
    height: 30px;
  }
  @keyframes prilet {
    from {
      opacity: 0;
      translate: 0 10px;
    }
  }
</style>
