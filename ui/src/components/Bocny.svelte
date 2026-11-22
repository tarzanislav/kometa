<script lang="ts">
  import { app, nastav, noc, type Stranka } from '../lib/app.svelte'
  import { pocet, rychlost } from '../lib/format'
  import type { MenoIkony } from '../lib/ikony'
  import Graf from './Graf.svelte'
  import Ikona from './Ikona.svelte'
  import Logo from './Logo.svelte'

  const aktivne = $derived(app.stav?.polozky.filter((p) => p.stav === 'stahuje' || p.stav === 'caka').length ?? 0)
  const bezi = $derived(app.stav?.polozky.filter((p) => p.stav === 'stahuje').length ?? 0)
  const chyby = $derived(app.stav?.polozky.filter((p) => p.stav === 'chyba').length ?? 0)
  const r = $derived(rychlost(app.stav?.rychlost ?? 0))

  const polozky: { id: Stranka; text: string; ikona: MenoIkony }[] = [
    { id: 'stahovanie', text: 'Sťahovanie', ikona: 'stiahnut' },
    { id: 'jazyky', text: 'Jazykové stopy', ikona: 'jazyky' },
    { id: 'ucty', text: 'Účty', ikona: 'ucty' },
    { id: 'nastavenia', text: 'Nastavenia', ikona: 'nastavenia' },
  ]

  const rezimy: { id: 'auto' | 'den' | 'noc'; text: string; ikona: MenoIkony }[] = [
    { id: 'auto', text: 'Podľa systému', ikona: 'auto' },
    { id: 'den', text: 'Deň', ikona: 'slnko' },
    { id: 'noc', text: 'Noc', ikona: 'mesiac' },
  ]

  const ucty = $derived(
    (['webshare', 'fastshare'] as const)
      .map((s) => ({ s, nazov: s === 'webshare' ? 'Webshare' : 'Fastshare', u: app.stav?.ucty[s] }))
      .filter((x) => x.u && x.u.stav !== 'neprihlaseny'),
  )
</script>

<nav class="bocny" aria-label="Hlavná ponuka">
  <div class="znacka">
    <Logo velkost={30} animovat />
    <span>Kometa</span>
  </div>

  <ul>
    {#each polozky as p (p.id)}
      <li>
        <button
          class="polozka"
          aria-current={app.stranka === p.id ? 'page' : undefined}
          onclick={() => (app.stranka = p.id)}
        >
          <Ikona meno={p.ikona} velkost={19} />
          <span>{p.text}</span>
          <span class="skryte">{p.text}</span>
          {#if p.id === 'stahovanie' && chyby}
            <span class="odznak chyba cisla" title="Chyby">{chyby}</span>
          {:else if p.id === 'stahovanie' && aktivne}
            <span class="odznak cisla">{aktivne}</span>
          {/if}
          {#if p.id === 'jazyky' && app.stav?.jazyky.busy}
            <span class="bodka" title="Sťahujú sa jazyky"></span>
          {/if}
        </button>
      </li>
    {/each}
  </ul>

  <div class="dole">
    <section class="rychlost" aria-label="Celková rýchlosť sťahovania">
      <div class="cislo cisla"><strong>{r.cislo}</strong> <span>{r.jednotka}</span></div>
      <Graf body={app.stav?.graf ?? []} />
      <p class="male tlmene">{bezi ? `${bezi >= 2 && bezi <= 4 ? 'Sťahujú' : 'Sťahuje'} sa ${pocet(bezi, 'súbor', 'súbory', 'súborov')}` : 'Nič sa nesťahuje'}</p>
    </section>

    {#if ucty.length}
      <button class="ucty" onclick={() => (app.stranka = 'ucty')}>
        {#each ucty as { s, nazov, u } (s)}
          <span class="ucet">
            <span class="svetlo {u?.stav}"></span>
            <span class="male">{nazov}</span>
            <span class="male tlmene skrat">{u?.stav === 'ok' ? 'VIP' : u?.stav === 'bez_vip' ? 'bez VIP' : u?.stav === 'chyba' ? 'chyba' : '…'}</span>
          </span>
        {/each}
      </button>
    {/if}

    <div class="rezim" role="radiogroup" aria-label="Vzhľad">
      {#each rezimy as m (m.id)}
        <button
          role="radio"
          aria-checked={(app.nastavenia?.rezim ?? 'auto') === m.id}
          title={m.text}
          aria-label={m.text}
          onclick={() => nastav({ rezim: m.id })}
        >
          <Ikona meno={m.ikona} velkost={17} />
        </button>
      {/each}
      <span class="verzia male tlmene cisla" title={noc() ? 'Nočný režim' : 'Denný režim'}>v{app.init?.verzia}</span>
    </div>
  </div>
</nav>

<style>
  .bocny {
    display: flex;
    flex-direction: column;
    width: var(--sidebar);
    flex: none;
    height: 100%;
    padding: var(--s5) var(--s3) var(--s4);
    border-right: 1px solid var(--line);
    background: var(--panel);
  }
  .znacka {
    display: flex;
    align-items: center;
    gap: var(--s3);
    padding: 0 var(--s3) var(--s6);
    font: 700 22px/1 var(--font-display);
    font-variation-settings: 'opsz' 36;
    letter-spacing: -0.02em;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .polozka {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--s3);
    width: 100%;
    height: 40px;
    padding: 0 var(--s3);
    border: 0;
    border-radius: var(--r-ctl);
    background: transparent;
    color: var(--ink-2);
    font: 600 var(--t-m) / 1 var(--font-text);
    text-align: left;
    cursor: pointer;
    transition: background-color 0.15s, color 0.15s;
  }
  .polozka:hover {
    background: var(--sunken);
    color: var(--ink);
  }
  .polozka[aria-current='page'] {
    background: var(--accent-soft);
    color: var(--accent-ink);
  }
  .polozka span:first-of-type {
    flex: 1;
  }
  .odznak {
    min-width: 22px;
    padding: 2px 7px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--on-accent);
    font-size: 11.5px;
    font-weight: 700;
    text-align: center;
  }
  .odznak.chyba {
    background: var(--err);
    color: oklch(99% 0 0);
  }
  .bodka {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--head);
    box-shadow: 0 0 8px var(--head-glow);
  }
  .dole {
    margin-top: auto;
    display: flex;
    flex-direction: column;
    gap: var(--s3);
  }
  .rychlost {
    padding: var(--s3) var(--s3) var(--s2);
    border-radius: var(--r-panel);
    background: var(--bg);
  }
  .cislo {
    display: flex;
    align-items: baseline;
    gap: 5px;
    font-family: var(--font-display);
  }
  .cislo strong {
    font-size: 30px;
    font-weight: 650;
    font-variation-settings: 'opsz' 60;
    letter-spacing: -0.02em;
    line-height: 1.05;
  }
  .cislo span {
    font-size: var(--t-s);
    color: var(--muted);
    font-family: var(--font-text);
    font-weight: 600;
  }
  .rychlost :global(svg) {
    margin: 6px 0 4px;
  }
  .ucty {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: var(--s2) var(--s3);
    border: 0;
    border-radius: var(--r-ctl);
    background: transparent;
    color: var(--ink);
    text-align: left;
    cursor: pointer;
  }
  .ucty:hover {
    background: var(--sunken);
  }
  .ucet {
    display: flex;
    align-items: center;
    gap: var(--s2);
  }
  .skrat {
    margin-left: auto;
  }
  .svetlo {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--faint);
  }
  .svetlo.ok {
    background: var(--ok);
  }
  .svetlo.bez_vip {
    background: var(--warn);
  }
  .svetlo.chyba {
    background: var(--err);
  }
  .rezim {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: var(--s2) var(--s1) 0;
    border-top: 1px solid var(--line);
  }
  .rezim button {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border: 0;
    border-radius: var(--r-ctl);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .rezim button:hover {
    color: var(--ink);
    background: var(--sunken);
  }
  .rezim button[aria-checked='true'] {
    color: var(--accent-ink);
    background: var(--accent-soft);
  }
  .verzia {
    margin-left: auto;
    padding-right: var(--s2);
  }
  /* úzke okno (a neskôr mobil): bočný panel len s ikonami */
  @media (max-width: 900px) {
    .bocny {
      width: 68px;
      padding: var(--s4) var(--s2);
      align-items: center;
    }
    .znacka {
      padding: 0 0 var(--s5);
    }
    .znacka span,
    .polozka span:first-of-type,
    .rychlost :global(svg),
    .rychlost p,
    .ucty,
    .verzia {
      display: none;
    }
    .polozka {
      justify-content: center;
      width: 44px;
      padding: 0;
    }
    .odznak {
      position: absolute;
      top: 2px;
      right: -2px;
      min-width: 18px;
      padding: 1px 5px;
      font-size: 10px;
    }
    .rychlost {
      padding: var(--s2) 4px;
      text-align: center;
    }
    .cislo {
      flex-direction: column;
      align-items: center;
      gap: 0;
    }
    .cislo strong {
      font-size: 17px;
    }
    .cislo span {
      font-size: 10px;
    }
    .rezim {
      flex-direction: column;
      border-top: 0;
    }
  }
</style>
