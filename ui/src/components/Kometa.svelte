<script lang="ts">
  // Priebeh ako kométa: svietiace jadro na čele, chvost za ním. Bez známej veľkosti preletí kométa dookola.
  let {
    hodnota = null,
    stav = 'bezi',
    hrubka = 6,
    popis = 'Priebeh',
  }: { hodnota?: number | null; stav?: 'bezi' | 'pauza' | 'hotovo' | 'chyba' | 'caka'; hrubka?: number; popis?: string } =
    $props()

  const percento = $derived(hodnota == null ? null : Math.max(0, Math.min(1, hodnota)) * 100)
</script>

<div
  class="drah {stav}"
  class:neurcite={percento == null && stav === 'bezi'}
  style:--hrubka="{hrubka}px"
  role="progressbar"
  aria-label={popis}
  aria-valuemin="0"
  aria-valuemax="100"
  aria-valuenow={percento == null ? undefined : Math.round(percento)}
>
  {#if stav !== 'caka'}
    <div class="chvost" style:width={percento == null ? undefined : `${percento}%`}>
      {#if stav === 'bezi' || stav === 'pauza'}<span class="jadro"></span>{/if}
    </div>
  {/if}
</div>

<style>
  .drah {
    position: relative;
    height: var(--hrubka);
    border-radius: 999px;
    background: var(--sunken);
  }
  .chvost {
    position: absolute;
    inset: 0 auto 0 0;
    min-width: calc(var(--hrubka) * 2);
    border-radius: inherit;
    background: linear-gradient(
      90deg,
      color-mix(in oklch, var(--accent) 8%, transparent),
      color-mix(in oklch, var(--accent) 55%, transparent) 55%,
      var(--accent) 88%,
      var(--head)
    );
    transition: width 0.45s var(--ease);
  }
  /* prúd svetla, ktorý tečie od jadra dozadu */
  .bezi .chvost::after {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: linear-gradient(90deg, transparent 0 30%, oklch(100% 0 0 / 0.5) 45%, transparent 60% 100%);
    background-size: 220% 100%;
    animation: prud 1.8s linear infinite;
    mix-blend-mode: soft-light;
  }
  .jadro {
    position: absolute;
    top: 50%;
    right: 0;
    width: calc(var(--hrubka) * 2.2);
    height: calc(var(--hrubka) * 2.2);
    border-radius: 50%;
    translate: 45% -50%;
    background: radial-gradient(circle, oklch(99% 0.02 var(--hb)) 0 28%, var(--head) 52%, transparent 72%);
    box-shadow: 0 0 calc(var(--hrubka) * 1.8) calc(var(--hrubka) * 0.4) var(--head-glow);
  }
  .bezi .jadro {
    animation: tep 1.8s ease-in-out infinite;
  }
  .pauza .chvost {
    background: linear-gradient(90deg, transparent, color-mix(in oklch, var(--warn) 45%, transparent) 60%, var(--warn));
  }
  .pauza .jadro {
    background: radial-gradient(circle, var(--warn) 0 45%, transparent 72%);
    box-shadow: none;
  }
  .hotovo .chvost {
    width: 100%;
    background: var(--accent);
  }
  .chyba .chvost {
    background: linear-gradient(90deg, transparent, var(--err));
  }
  .neurcite .chvost {
    width: 28%;
    animation: prelet 1.6s var(--ease) infinite;
  }
  @keyframes prud {
    from {
      background-position: 0% 0;
    }
    to {
      background-position: 220% 0;
    }
  }
  @keyframes tep {
    50% {
      box-shadow: 0 0 calc(var(--hrubka) * 2.6) calc(var(--hrubka) * 0.6) var(--head-glow);
    }
  }
  @keyframes prelet {
    from {
      left: -28%;
    }
    to {
      left: 100%;
    }
  }
</style>
