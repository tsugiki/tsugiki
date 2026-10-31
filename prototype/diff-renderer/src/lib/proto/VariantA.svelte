<!-- PROTOTYPE — throwaway. A: every row highlighted eagerly in Rust, sent once, virtualized per row in JS. -->
<script lang="ts">
  import VirtualRows from "./VirtualRows.svelte";
  import type { LoadInfo, Row } from "./types";

  export const name = "Row virtualizer · all rows in JS";

  interface Props {
    info: LoadInfo;
    rows: Row[];
    collapsed: Set<number>;
    onToggle: (file: number) => void;
    scroller?: HTMLElement;
  }
  let { info, rows, collapsed, onToggle, scroller = $bindable() }: Props = $props();
</script>

<VirtualRows {info} {collapsed} {onToggle} getRow={(id) => rows[id]} bind:scroller />
