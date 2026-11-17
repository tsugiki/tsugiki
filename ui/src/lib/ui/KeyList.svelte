<!-- The list `?` opens (#559): the keys that work where the press went, each with its action's title, from
     the core's key table with the user's remaps. The view's own keys come first, then those that work
     anywhere. It closes on Esc, Close or a press outside, and gives the focus back. -->
<script lang="ts" module>
  import type { Action, Context } from "$lib/state/keymap.svelte";

  /** A view whose keys work only there, which the list names. The composer has none to list yet. */
  export type View = Exclude<Context, "anywhere" | "composer">;

  /** Which keys work where a press went, as worked out at the press: the views it went to, whose keys the
   * list shows before those that work anywhere, and the actions whose key runs them there. */
  export interface KeysHere {
    views: readonly View[];
    actions: ReadonlySet<Action>;
  }

  /** What the Repository's screen says of a press: the views it went to, and whether each of the screen's
   * actions would run. It says yes of any other action, which the page decides on. */
  export interface ScreenKeys {
    views: readonly View[];
    works: (action: Action) => boolean;
  }
</script>

<script lang="ts">
  import { CONTEXT_NAMES, keysIn } from "$lib/state/keymap.svelte";
  import Dialog from "./Dialog.svelte";
  import { bindingLabel, currentOs } from "./keys";

  let { here, onclose }: { here: KeysHere; onclose: () => void } = $props();

  const os = currentOs();
  /** Each view's keys that work, and a view with none goes. */
  const groups = $derived(
    [...here.views, "anywhere" as const]
      .map((context) => ({
        heading: CONTEXT_NAMES[context],
        rows: keysIn(context).filter((row) => here.actions.has(row.action)),
      }))
      .filter((group) => group.rows.length > 0),
  );
</script>

<Dialog label="Keys" testid="key-list" size="narrow" {onclose}>
  <h2>Keys</h2>
  {#each groups as { heading, rows } (heading)}
    <section aria-label={heading}>
      <h3>{heading}</h3>
      <dl>
        {#each rows as row (row.action)}
          <dt><kbd>{bindingLabel(row.key, os)}</kbd></dt>
          <dd>{row.title}</dd>
        {/each}
      </dl>
    </section>
  {/each}
  <div class="buttons">
    <button onclick={onclose}>Close</button>
  </div>
</Dialog>

<style>
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
