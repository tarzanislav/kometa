<script lang="ts">
  import { app, nastav, skus, toast } from '../lib/app.svelte'
  import type { StavUctu } from '../lib/typy'
  import Ikona from '../components/Ikona.svelte'

  type Sluzba = 'webshare' | 'fastshare'
  const sluzby: { id: Sluzba; nazov: string; popis: string }[] = [
    { id: 'webshare', nazov: 'Webshare.cz', popis: 'Na rýchle sťahovanie treba VIP.' },
    { id: 'fastshare', nazov: 'Fastshare.cz', popis: 'Sťahuje sa z kreditu na tvojom účte.' },
  ]
  const formular = $state<Record<Sluzba, { meno: string; heslo: string; pracuje: boolean }>>({
    webshare: { meno: app.stav?.ucty.webshare.user ?? '', heslo: '', pracuje: false },
    fastshare: { meno: app.stav?.ucty.fastshare.user ?? '', heslo: '', pracuje: false },
  })

  const NAZVY: Record<string, string> = {
    firefox: 'Firefox',
    chrome: 'Chrome',
    edge: 'Edge',
    brave: 'Brave',
    opera: 'Opera',
    vivaldi: 'Vivaldi',
    chromium: 'Chromium',
    safari: 'Safari',
  }

  async function prihlasit(s: Sluzba) {
    const f = formular[s]
    f.pracuje = true
    const r = await skus<StavUctu>('prihlasit', s, f.meno, f.heslo)
    f.pracuje = false
    if (r) {
      f.heslo = ''
      toast(`Prihlásený do ${s === 'webshare' ? 'Webshare' : 'Fastshare'}.`, 'ok')
    }
  }

  async function odhlasit(s: Sluzba) {
    await skus('odhlasit', s)
    formular[s].meno = ''
    formular[s].heslo = ''
  }
</script>

<div class="stranka">
  <header>
    <h1>Účty</h1>
    <p class="tlmene">Heslá sa ukladajú do systémovej kľúčenky v tomto počítači a nikam inam sa neposielajú.</p>
  </header>

  <div class="mriezka">
    {#each sluzby as s (s.id)}
      {@const u = app.stav?.ucty[s.id]}
      {@const prihlaseny = u && u.stav !== 'neprihlaseny'}
      <section class="plocha ucet" aria-labelledby="ucet-{s.id}">
        <div class="hlava">
          <span class="ikona"><Ikona meno="oblak" velkost={22} /></span>
          <div>
            <h2 id="ucet-{s.id}">{s.nazov}</h2>
            <p class="tlmene male">{s.popis}</p>
          </div>
        </div>

        {#if prihlaseny}
          <div class="stav {u!.stav}">
            <span class="svetlo"></span>
            <div>
              <strong>{u!.user}</strong>
              <p class="male vyber-text">{u!.text || (u!.stav === 'overujem' ? 'Overujem účet…' : '')}</p>
            </div>
          </div>
          <button class="tl" onclick={() => odhlasit(s.id)}>Odhlásiť</button>
        {:else}
          <form
            onsubmit={(e) => {
              e.preventDefault()
              prihlasit(s.id)
            }}
          >
            <div>
              <label class="popis" for="{s.id}-meno">Meno</label>
              <input id="{s.id}-meno" class="pole" bind:value={formular[s.id].meno} autocomplete="username" />
            </div>
            <div>
              <label class="popis" for="{s.id}-heslo">Heslo</label>
              <input
                id="{s.id}-heslo"
                class="pole"
                type="password"
                bind:value={formular[s.id].heslo}
                autocomplete="current-password"
              />
            </div>
            <button class="tl hlavne" disabled={!formular[s.id].meno || formular[s.id].pracuje}>
              {formular[s.id].pracuje ? 'Prihlasujem…' : 'Prihlásiť'}
            </button>
          </form>
          {#if u?.stav === 'chyba' && u.text}<p class="chyba male">{u.text}</p>{/if}
        {/if}
      </section>
    {/each}
  </div>

  <section class="plocha prehliadac" aria-labelledby="h-prehliadac">
    <div class="hlava">
      <span class="ikona"><Ikona meno="zamok" velkost={22} /></span>
      <div>
        <h2 id="h-prehliadac">Stránky po prihlásení</h2>
        <p class="tlmene male">
          Instagram, Facebook, platené kurzy alebo súkromné Vimeo: prihlás sa na stránke vo svojom prehliadači a vyber
          ho tu. Kometa pri sťahovaní použije jeho prihlásenie. Najspoľahlivejší je Firefox; Chrome a Edge treba pri
          sťahovaní zavrieť.
        </p>
      </div>
    </div>
    <div class="vyber-prehliadaca">
      <label class="popis" for="prehliadac">Prehliadač</label>
      <select
        id="prehliadac"
        class="vyber"
        value={app.nastavenia?.cookies_browser ?? ''}
        onchange={(e) => nastav({ cookies_browser: (e.currentTarget as HTMLSelectElement).value })}
      >
        <option value="">Nepoužívať</option>
        {#each app.init?.prehliadace ?? [] as p (p)}<option value={p}>{NAZVY[p] ?? p}</option>{/each}
      </select>
    </div>
  </section>

  <p class="tlmene male poznamka">Kometa neobchádza obmedzenia bezplatných účtov ani ochranu obsahu (DRM).</p>
</div>

<style>
  .stranka {
    max-width: 1000px;
    padding: var(--s8) var(--s8) var(--s10);
  }
  header p {
    margin-top: var(--s1);
  }
  .mriezka {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: var(--s4);
    margin-top: var(--s6);
  }
  .ucet,
  .prehliadac {
    display: flex;
    flex-direction: column;
    gap: var(--s4);
    padding: var(--s5);
  }
  .ucet > .tl {
    align-self: flex-start;
  }
  .prehliadac {
    margin-top: var(--s4);
  }
  .hlava {
    display: flex;
    gap: var(--s3);
    align-items: flex-start;
  }
  .hlava p {
    margin-top: 2px;
    max-width: 70ch;
  }
  .ikona {
    display: grid;
    place-items: center;
    flex: none;
    width: 42px;
    height: 42px;
    border-radius: 12px;
    background: var(--accent-soft);
    color: var(--accent-ink);
  }
  form {
    display: flex;
    flex-direction: column;
    gap: var(--s3);
  }
  form .tl {
    align-self: flex-start;
  }
  .stav {
    display: flex;
    gap: var(--s3);
    align-items: flex-start;
    padding: var(--s3) var(--s4);
    border-radius: var(--r-ctl);
    background: var(--sunken);
  }
  .stav.ok {
    background: var(--ok-soft);
  }
  .stav.bez_vip {
    background: var(--warn-soft);
  }
  .stav.chyba {
    background: var(--err-soft);
  }
  .svetlo {
    flex: none;
    width: 9px;
    height: 9px;
    margin-top: 6px;
    border-radius: 50%;
    background: var(--faint);
  }
  .ok .svetlo {
    background: var(--ok);
  }
  .bez_vip .svetlo {
    background: var(--warn);
  }
  .chyba .svetlo {
    background: var(--err);
  }
  .chyba {
    color: var(--err);
    font-weight: 600;
  }
  .vyber-prehliadaca {
    max-width: 260px;
  }
  .poznamka {
    margin-top: var(--s5);
  }
  @media (max-width: 860px) {
    .stranka {
      padding: var(--s5) var(--s4) var(--s8);
    }
  }
</style>
