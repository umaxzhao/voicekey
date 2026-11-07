<script lang="ts">
  import { app } from "./store.svelte";
  const N = 24;
  const n = $derived(Math.round(app.level * N));
</script>

<div class="meter" aria-hidden="true">
  {#each Array(N) as _, i}
    <i style={`--i:${i}`} class:lit={i < n}></i>
  {/each}
</div>

<style>
  .meter { display: flex; gap: 3px; align-items: flex-end; height: 26px; width: 200px; }
  i {
    flex: 1; height: 20%; border-radius: 3px; background: var(--line);
    transition: height 0.08s linear, background 0.15s, box-shadow 0.15s;
  }
  i.lit {
    height: calc(25% + var(--i) * 3.2%);
    background: linear-gradient(180deg, #3ee7ff, #7c6bff 55%, #ff5fa2);
    box-shadow: 0 0 8px rgba(124, 107, 255, 0.3);
  }
</style>
