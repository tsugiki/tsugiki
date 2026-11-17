<!-- A line of code in pieces: highlighted by its spans, with the word `picked` marked, and a caret where the
     pick has one. Each piece sits on one line, as the row keeps its text's spaces. window.css styles the
     caret and the pick. -->
<script lang="ts">
  import type { Span } from "$lib/state/pr-files.svelte";
  import type { Picked } from "$lib/state/pick";
  import { pieces } from "./spans";

  let { text, spans, picked }: { text: string; spans: Span[]; picked?: Picked | undefined } = $props();
</script>

<!-- prettier-ignore -->
{#each pieces(text, spans, [], picked) as piece, at (at)}{#if piece.caret}<span class="caret"></span>{/if}{#if piece.highlight || piece.picked}<span class={[piece.highlight && `hl-${piece.highlight}`, piece.picked && "picked"]}>{piece.text}</span>{:else}{piece.text}{/if}{/each}
