<!-- The list `?` opens (#559): the keys that work where the focus was, each with its action's title, from the
     core's key table with the user's remaps. The view's own keys come first, then those that work anywhere.
     It closes on Esc, Close or a press outside, and gives the focus back. -->
<script lang="ts" module>
  /** A view whose keys work only there, which the list names. */
  export type View = "issue_list" | "diff";
</script>

<script lang="ts">
  import { keysIn } from "$lib/state/keymap.svelte";
  import { closeOnPress, takeFocus } from "./focus";
  import { actionLabel, currentOs } from "./keys";

  let {
    views,
    development,
    onclose,
  }: {
    /** The views the focus was in, whose keys the list shows before those that work anywhere. */
    views: readonly View[];
    /** Whether this is a development build, the only one in which Open DevTools has a key. */
    development: boolean;
    onclose: () => void;
  } = $props();

  const HEADINGS: Readonly<Record<View | "anywhere", string>> = {
    issue_list: "Issue list",
    diff: "Diff",
    anywhere: "Anywhere",
  };

  const os = currentOs();
  const groups = $derived(
    [...views, "anywhere" as const].map((context) => ({
      heading: HEADINGS[context],
      rows: keysIn(context).filter((row) => development || row.action !== "open_devtools"),
    })),
  );

  function closeOnEscape(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={closeOnEscape} />

<!-- The keyboard's way out is Esc, on the window above. This is the mouse's. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" data-testid="key-list-scrim" onmousedown={closeOnPress(onclose)}>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label="Keys"
    tabindex="-1"
    {@attach takeFocus}
    onmousedown={(e) => {
      e.stopPropagation();
    }}
  >
    <h2>Keys</h2>
    {#each groups as { heading, rows } (heading)}
      <section aria-label={heading}>
        <h3>{heading}</h3>
        <dl>
          {#each rows as row (row.action)}
            <dt><kbd>{actionLabel(row.action, os)}</kbd></dt>
            <dd>{row.title}</dd>
          {/each}
        </dl>
      </section>
    {/each}
    <div class="buttons">
      <button onclick={onclose}>Close</button>
    </div>
  </div>
</div>

<style>
  .dialog {
    width: min(560px, calc(100vw - 32px));
    max-height: calc(100vh - 120px);
    margin: 60px auto 0;
    padding: 12px;
    box-sizing: border-box;
    overflow: auto;
    font-size: 13px;
    background: var(--bg-overlay);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 8px 30px var(--shadow);
  }

  .dialog:focus {
    outline: none;
  }

  h2 {
    margin: 0;
    font-size: 14px;
  }

  h3 {
    margin: 12px 0 4px;
    font-size: 12px;
    font-weight: normal;
    color: var(--fg-muted);
  }

  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 16px;
    margin: 0;
  }

  dt,
  dd {
    margin: 0;
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    margin-top: 12px;
  }
</style>
