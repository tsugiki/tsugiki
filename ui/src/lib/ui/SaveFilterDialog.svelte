<!-- Save a search as a Filter (#471): a Name, the query by default, and a Query with Tsugiki's additions
     spelled out. Save is off while either is empty, and the core refuses a name the Repository's Filters
     already use, which the dialog says. #472 opens it with the login as the name. -->
<script lang="ts">
  import Dialog from "./Dialog.svelte";
  import { untrack } from "svelte";
  import { takeFocus } from "./focus";

  let {
    name: defaultName,
    query: defaultQuery,
    onsave,
    onclose,
  }: {
    /** The name the dialog opens with. */
    name: string;
    /** The query the dialog opens with, with Tsugiki's additions spelled out. */
    query: string;
    /** Saves the Filter, answering the core's reason when it refuses one, or nothing when it saved. */
    onsave: (name: string, query: string) => Promise<string | undefined>;
    onclose: () => void;
  } = $props();

  // The dialog's own fields: it opens with the draft's name and query, and edits them on their own. A
  // refusal speaks of what the fields held, so an edit drops it. While the save is in flight they take no
  // edits, so an answer lands on the values it speaks of, and they keep the focus a disabled field loses.
  let name = $state(untrack(() => defaultName));
  let query = $state(untrack(() => defaultQuery));
  let problem = $state<string>();
  let saving = $state(false);
  const incomplete = $derived(name.trim() === "" || query.trim() === "");

  function edited() {
    problem = undefined;
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (incomplete || saving) return;
    saving = true;
    problem = await onsave(name.trim(), query.trim());
    saving = false;
    if (problem === undefined) onclose();
  }
</script>

<Dialog label="Save as Filter" testid="save-filter" size="wide" childTakesFocus {onclose}>
  <form onsubmit={submit}>
    <label>
      Name
      <input
        {@attach takeFocus}
        bind:value={name}
        autocomplete="off"
        spellcheck="false"
        readonly={saving}
        oninput={edited}
      />
    </label>
    <label>
      Query
      <input bind:value={query} autocomplete="off" spellcheck="false" readonly={saving} oninput={edited} />
    </label>
    {#if problem}
      <p role="alert">{problem}</p>
    {/if}
    <div class="buttons">
      <button type="button" onclick={onclose}>Cancel</button>
      <button type="submit" disabled={incomplete || saving}>Save</button>
    </div>
  </form>
</Dialog>

<style>
  label {
    display: flex;
    gap: 8px;
    align-items: baseline;
  }

  input {
    flex: 1;
    padding: 6px 8px;
    font-size: 14px;
  }

  p {
    margin: 8px 0 0;
    overflow-wrap: anywhere;
  }

  .buttons {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    margin-top: 12px;
  }
</style>
