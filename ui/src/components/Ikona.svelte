<script lang="ts">
  import { IKONY, type MenoIkony } from '../lib/ikony'

  let { meno, velkost = 18, hrubka = 1.75 }: { meno: MenoIkony; velkost?: number; hrubka?: number } = $props()
</script>

<svg
  width={velkost}
  height={velkost}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={meno === 'uchop' ? 3 : hrubka}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
><path d={IKONY[meno]} /></svg>

<style>
  svg {
    flex: none;
    display: block;
  }
</style>
