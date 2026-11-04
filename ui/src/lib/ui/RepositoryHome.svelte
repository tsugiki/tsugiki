<script lang="ts">
  // Where the Workbench goes when it can sync. Until then this shows that the launch reopened the Repository.
  let { name, onadd }: { name: string; onadd: () => void } = $props();
</script>

<section>
  <h1>{name}</h1>
  <p>This Repository's Workbench opens here once Tsugiki can sync.</p>
  <button onclick={onadd}>Add repository</button>
</section>

<style>
  h1 {
    font-size: 16px;
  }
</style>
