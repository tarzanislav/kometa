<script lang="ts">
  // Kometa: jadro s chvostom. Pri spustení sa chvost raz „dokreslí“ – jediná animácia, ktorá nie je odpoveďou na čin.
  let { velkost = 30, animovat = false }: { velkost?: number; animovat?: boolean } = $props()
  const id = `chvost-${Math.random().toString(36).slice(2, 8)}`
</script>

<svg width={velkost} height={velkost} viewBox="0 0 32 32" aria-hidden="true" class:animovat>
  <defs>
    <linearGradient id={id} gradientUnits="userSpaceOnUse" x1="3" y1="29" x2="23" y2="9">
      <stop offset="0" stop-color="var(--accent)" stop-opacity="0" />
      <stop offset="0.7" stop-color="var(--accent)" stop-opacity="0.85" />
      <stop offset="1" stop-color="var(--head)" />
    </linearGradient>
  </defs>
  <g fill="none" stroke={`url(#${id})`} stroke-linecap="round">
    <path class="t t1" d="M3.5 28.5C9.5 23 15 17 21 11.5" stroke-width="3.4" />
    <path class="t t2" d="M9 30C14.5 25.5 18.5 20 22.5 13.5" stroke-width="1.5" />
    <path class="t t3" d="M2 22.5C7.5 19 13 15 19.5 10.5" stroke-width="1.5" />
  </g>
  <circle class="jadro" cx="22.6" cy="9.4" r="5.2" fill="var(--head)" />
  <circle cx="22.6" cy="9.4" r="2.3" fill="oklch(99% 0.02 var(--hb))" />
</svg>

<style>
  svg {
    display: block;
    flex: none;
    overflow: visible;
  }
  .jadro {
    filter: drop-shadow(0 0 3px var(--head-glow));
  }
  .animovat .t {
    stroke-dasharray: 40;
    stroke-dashoffset: 40;
    animation: kresli 0.9s var(--ease) 0.15s forwards;
  }
  .animovat .t2 {
    animation-delay: 0.3s;
  }
  .animovat .t3 {
    animation-delay: 0.38s;
  }
  .animovat .jadro {
    animation: zazar 0.9s var(--ease);
  }
  @keyframes kresli {
    to {
      stroke-dashoffset: 0;
    }
  }
  @keyframes zazar {
    from {
      opacity: 0;
      transform: scale(0.4);
      transform-origin: 22.6px 9.4px;
    }
  }
</style>
