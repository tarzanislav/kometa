<script lang="ts">
  // Graf rýchlosti za posledných 90 sekúnd.
  let { body }: { body: number[] } = $props()

  const W = 90
  const H = 28
  const cesty = $derived.by(() => {
    const n = body.length
    if (n < 2) return { ciara: '', plocha: '' }
    const max = Math.max(...body, 1)
    const pts = body.map((v, i) => [(i / (n - 1)) * W, H - 2 - (v / max) * (H - 4)])
    const ciara = pts.map(([x, y], i) => `${i ? 'L' : 'M'}${x.toFixed(1)} ${y.toFixed(1)}`).join('')
    return { ciara, plocha: `${ciara}L${W} ${H}L0 ${H}Z` }
  })
</script>

<svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" aria-hidden="true">
  <path d={cesty.plocha} class="plocha" />
  <path d={cesty.ciara} class="ciara" />
</svg>

<style>
  svg {
    display: block;
    width: 100%;
    height: 34px;
  }
  .plocha {
    fill: color-mix(in oklch, var(--accent) 16%, transparent);
  }
  .ciara {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.5;
    vector-effect: non-scaling-stroke;
    stroke-linejoin: round;
  }
</style>
