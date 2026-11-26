<script lang="ts">
  // Titulky k jazykom: vypnuté, alebo ku každému jazyku zvuku titulky v tom istom jazyku.
  import Prepinac from './Prepinac.svelte'
  import Segment from './Segment.svelte'

  let {
    titulky = $bindable('vypnute'),
    auto = $bindable(false),
    mp4 = false,
  }: { titulky?: 'vypnute' | 'jazyky'; auto?: boolean; mp4?: boolean } = $props()
</script>

<div class="titulky">
  <span class="popis">Titulky</span>
  <Segment
    popis="Titulky"
    bind:hodnota={titulky}
    moznosti={[
      { id: 'vypnute', text: 'Bez titulkov' },
      { id: 'jazyky', text: 'K jazykom zvuku' },
    ]}
  />
  {#if titulky === 'jazyky'}
    <p class="male tlmene">
      Ku každému jazyku zvuku titulky v tom istom jazyku, vložené do súboru{mp4
        ? ' ako titulky MP4 (prehrajú ich aj telefóny a televízory)'
        : ''} – predvolene vypnuté, zapneš ich v prehrávači.
    </p>
    <Prepinac
      nazov="Aj automatické titulky"
      bind:zapnute={auto}
      popis="Keď autor videa titulky nedal, použijú sa automatické od YouTube (strojovo rozpoznané alebo preložené)."
    />
  {/if}
</div>

<style>
  .titulky {
    display: flex;
    flex-direction: column;
    gap: var(--s2);
  }
</style>
