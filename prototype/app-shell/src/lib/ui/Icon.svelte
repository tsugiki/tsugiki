<script lang="ts">
  // PROTOTYPE — throwaway. Line icons for the Sidebar's icon rail, drawn on a 16×16 grid.
  let { name }: { name: string } = $props();
  const P: Record<string, string> = {
    inbox: "M2 9h3l1 2h4l1-2h3M2 9l2-6h8l2 6v4H2z",
    eye: "M1 8s2.5-5 7-5 7 5 7 5-2.5 5-7 5-7-5-7-5z",
    clock: "M8 4.5V8l2.5 1.5",
    bell: "M4 11V7a4 4 0 0 1 8 0v4l1 1.5H3zM6.5 14h3",
    archive: "M2 3h12v3H2zM3 6v7h10V6M6.5 9h3",
    mute: "M2 6h3l3-3v10L5 10H2zM11 6l4 4M15 6l-4 4",
    issue: "",
    filter: "M2 3h12L9.5 8.5V13l-3 1.5V8.5z",
    star: "M8 1.8l1.9 3.9 4.3.6-3.1 3 .7 4.3L8 11.6l-3.8 2 .7-4.3-3.1-3 4.3-.6z",
    gear: "M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4",
    home: "M2 8l6-5.5L14 8M3.5 7v6.5h3.5v-4h2v4h3.5V7",
    expand: "M6 3l5 5-5 5",
    collapse: "M10 3L5 8l5 5",
  };
  const circle: Record<string, [number, number]> = { eye: [2, 0], clock: [6, 0], issue: [6, 1.3], gear: [2.2, 0] };
</script>

<svg class="i" viewBox="0 0 16 16" aria-hidden="true">
  {#if P[name]}<path d={P[name]} />{/if}
  {#if circle[name]}<circle cx="8" cy="8" r={circle[name][0]} />{#if circle[name][1]}<circle class="f" cx="8" cy="8" r={circle[name][1]} />{/if}{/if}
</svg>

<style>
  .i { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 1.4; stroke-linecap: round; stroke-linejoin: round; flex: none; }
  .f { fill: currentColor; stroke: none; }
</style>
