<script lang="ts">
  // Prázdny front: kométa letí cez nočnú oblohu.
  const hviezdy = [
    [38, 22, 1.4], [92, 64, 1], [150, 18, 1.2], [214, 52, 0.9], [268, 14, 1.5], [322, 70, 1.1], [372, 30, 1],
    [60, 98, 0.9], [404, 92, 1.3], [120, 120, 1.1], [300, 118, 0.8], [240, 96, 1],
  ]
</script>

<svg viewBox="0 0 440 140" aria-hidden="true">
  <defs>
    <linearGradient id="prazdne-chvost" gradientUnits="userSpaceOnUse" x1="40" y1="128" x2="330" y2="40">
      <stop offset="0" stop-color="var(--accent)" stop-opacity="0" />
      <stop offset="0.75" stop-color="var(--accent)" stop-opacity="0.6" />
      <stop offset="1" stop-color="var(--head)" />
    </linearGradient>
  </defs>
  {#each hviezdy as [x, y, r], i (i)}
    <circle cx={x} cy={y} r={r} class="hviezda" style:animation-delay="{(i * 0.37) % 3}s" />
  {/each}
  <g fill="none" stroke="url(#prazdne-chvost)" stroke-linecap="round">
    <path d="M40 128C130 96 230 66 326 42" stroke-width="7" />
    <path d="M90 136C170 108 250 78 330 50" stroke-width="2" opacity="0.7" />
    <path d="M30 108C120 84 220 58 318 38" stroke-width="2" opacity="0.7" />
  </g>
  <circle cx="334" cy="40" r="12" fill="var(--head)" class="jadro" />
  <circle cx="334" cy="40" r="5" fill="oklch(99% 0.02 var(--hb))" />
</svg>

<style>
  svg {
    display: block;
    width: min(440px, 100%);
    height: auto;
  }
  .hviezda {
    fill: var(--faint);
    animation: blik 3.2s ease-in-out infinite;
  }
  .jadro {
    filter: drop-shadow(0 0 10px var(--head-glow));
  }
  @keyframes blik {
    50% {
      opacity: 0.25;
    }
  }
</style>
