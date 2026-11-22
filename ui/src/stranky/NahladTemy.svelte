<script lang="ts">
  // Zmenšené okno Komety vo zvolenej téme – farby sa počítajú z tej istej témy ako celé rozhranie.
  let { tema, noc }: { tema: string; noc: boolean } = $props()
</script>

<div class="mini" data-tema={tema} data-rezim={noc ? 'noc' : 'den'} aria-hidden="true">
  <div class="bok">
    <span class="logo"></span>
    <span class="nav aktivna"></span>
    <span class="nav"></span>
    <span class="nav"></span>
  </div>
  <div class="obsah">
    <span class="nadpis"></span>
    <span class="vstup"><span class="tlac"></span></span>
    <span class="riadok"><span class="obr"></span><span class="drah"><span class="chvost"></span></span></span>
    <span class="riadok"><span class="obr"></span><span class="drah"><span class="chvost kratky"></span></span></span>
  </div>
</div>

<style>
  .mini {
    display: flex;
    height: 112px;
    overflow: hidden;
    border-radius: 10px 10px 0 0;
    background: var(--bg);
  }
  .bok {
    display: flex;
    flex-direction: column;
    gap: 5px;
    width: 30%;
    padding: 10px 8px;
    background: var(--panel);
    border-right: 1px solid var(--line);
  }
  .logo {
    width: 14px;
    height: 14px;
    margin-bottom: 4px;
    border-radius: 50%;
    background: radial-gradient(circle, var(--head) 0 45%, transparent 70%);
    box-shadow: -6px 6px 8px -2px color-mix(in oklch, var(--accent) 60%, transparent);
  }
  .nav {
    height: 8px;
    border-radius: 4px;
    background: var(--sunken);
  }
  .nav.aktivna {
    background: var(--accent-soft);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .obsah {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 7px;
    padding: 10px;
  }
  .nadpis {
    width: 46%;
    height: 9px;
    border-radius: 4px;
    background: var(--ink);
    opacity: 0.85;
  }
  .vstup {
    display: flex;
    justify-content: flex-end;
    height: 20px;
    padding: 3px;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--panel);
  }
  .tlac {
    width: 28%;
    border-radius: 4px;
    background: var(--accent);
  }
  .riadok {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .obr {
    width: 22px;
    height: 13px;
    border-radius: 3px;
    background: var(--sunken);
  }
  .drah {
    flex: 1;
    height: 4px;
    border-radius: 2px;
    background: var(--sunken);
  }
  .chvost {
    display: block;
    position: relative;
    width: 70%;
    height: 100%;
    border-radius: 2px;
    background: linear-gradient(90deg, transparent, var(--accent) 85%, var(--head));
  }
  .chvost::after {
    content: '';
    position: absolute;
    right: -3px;
    top: -2px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--head);
    box-shadow: 0 0 6px var(--head-glow);
  }
  .chvost.kratky {
    width: 38%;
  }
</style>
