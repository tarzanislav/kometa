<script lang="ts">
  // Potvrdenie pred mazaním: celý front (súbory ostanú) alebo stiahnuté súbory do Koša.
  import { app, skus, toast } from '../lib/app.svelte'
  import { pocet, velkost } from '../lib/format'
  import Okno from '../components/Okno.svelte'

  let { co, ids = null }: { co: 'front' | 'subory'; ids?: string[] | null } = $props()

  let info = $state<{ pocet: number; velkost: number } | null>(null)
  let bezi = $state(false)
  const vsetko = $derived(ids == null)

  $effect(() => {
    if (co === 'subory') skus<{ pocet: number; velkost: number }>('subory_na_zmazanie', ids).then((r) => (info = r ?? null))
  })

  const zavriet = () => (app.dialog = null)

  async function potvrdit() {
    bezi = true
    if (co === 'front') {
      await skus('vycistit_front')
      toast('Front je prázdny. Stiahnuté súbory ostali na disku.', 'ok')
    } else {
      const r = await skus<{ zmazane: number; chyby: string[] }>('zmazat_subory', ids)
      if (r?.chyby.length) toast(`Do Koša sa nepodarilo presunúť: ${r.chyby.join(', ')}`, 'chyba')
      else if (r) toast(r.zmazane ? `Presunuté do Koša: ${pocet(r.zmazane, 'súbor', 'súbory', 'súborov')}.` : 'Hotovo.', 'ok')
    }
    bezi = false
    zavriet()
  }

  const nazov = $derived(
    co === 'front' ? 'Vyčistiť front?' : vsetko ? 'Vymazať stiahnuté súbory?' : 'Odstrániť aj súbor?',
  )
</script>

<Okno {nazov} sirka={520} {zavriet}>
  {#if co === 'front'}
    <p>
      Z frontu sa odstránia všetky položky. Čo sa práve sťahuje alebo čaká, sa zruší. Stiahnuté súbory ostanú na disku.
    </p>
  {:else if !info}
    <p class="tlmene">Zisťujem, čo je na disku…</p>
  {:else if info.pocet === 0}
    <p>Na disku už nie je žiadny súbor z {vsetko ? 'frontu' : 'tejto položky'}. {vsetko ? 'Front sa vyčistí.' : 'Položka sa odstráni zo zoznamu.'}</p>
  {:else}
    <p>
      Do Koša sa presunie <strong>{pocet(info.pocet, 'súbor', 'súbory', 'súborov')} ({velkost(info.velkost)})</strong>
      {vsetko ? 'stiahnutých Kometou a front sa vyčistí.' : 'a položka sa odstráni zo zoznamu.'}
    </p>
    <p class="tlmene male">
      Iné súbory v priečinku ostanú. Z Koša sa dajú obnoviť.
    </p>
  {/if}

  {#snippet pata()}
    <span class="medzera"></span>
    <button class="tl" onclick={zavriet}>Zrušiť</button>
    <button class="tl hlavne zmazat" disabled={bezi || (co === 'subory' && !info)} onclick={potvrdit}>
      {co === 'front' ? 'Vyčistiť front' : info?.pocet ? 'Presunúť do Koša' : 'Odstrániť'}
    </button>
  {/snippet}
</Okno>

<style>
  p + p {
    margin-top: var(--s2);
  }
  .medzera {
    flex: 1;
  }
  .zmazat {
    --tl-bg: var(--err);
    --tl-line: var(--err);
    --tl-fg: white;
  }
  .zmazat:hover:not(:disabled) {
    --tl-bg: var(--err);
    filter: brightness(1.08);
  }
</style>
