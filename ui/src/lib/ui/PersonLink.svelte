<!-- A login that fills the Inbox's search box with the search for what that person authored (#472). It reads
     as the text around it, and sits above a row's title, whose cover takes a click anywhere on the row. -->
<script lang="ts">
  let {
    login,
    onopen,
  }: {
    login: string;
    /** Fills the Inbox's search box with `login`'s search (#472). */
    onopen: (login: string) => void;
  } = $props();
</script>

<button type="button" class="link person" onclick={() => onopen(login)}>{login}</button>

<style>
  /* Above a row's title's cover, which otherwise takes the click. */
  .person {
    position: relative;
    z-index: 1;
  }
</style>
