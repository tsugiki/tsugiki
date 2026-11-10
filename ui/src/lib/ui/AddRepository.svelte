<script lang="ts">
  import { AddRepositoryState } from "$lib/state/add-repository.svelte";
  import type { Home } from "$lib/state/home.svelte";
  import { takeFocus } from "./focus";
  import MirrorChoices from "./MirrorChoices.svelte";
  import { describeCheck } from "./repository-check";

  let {
    onclose,
    onadded,
    onswitch,
  }: {
    onclose: () => void;
    onadded: (home: Home) => void;
    /** The user chose to switch to `name`, a Repository Tsugiki has already. */
    onswitch: (name: string) => void;
  } = $props();

  const adding = new AddRepositoryState();
  const step = $derived(adding.step);
  const check = $derived(step.step === "checked" || step.step === "adding" ? step.check : undefined);
  const waiting = $derived(step.step === "checking" || step.step === "adding");
  // A clone that can't lend, or one still being picked, leaves nothing to add with until the user decides.
  const undecided = $derived(check !== undefined && adding.mirror.source === undefined);
  const action = $derived(step.step === "held" ? "Switch to it" : check ? "Add" : "Check");

  // The one button does the next thing, so Enter takes the user from typing to checked to added, or to the
  // Repository Tsugiki has already.
  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (step.step === "held") {
      onswitch(step.name);
      return;
    }
    if (check === undefined) {
      await adding.check();
      return;
    }
    const home = await adding.add(check);
    if (home !== undefined) {
      onadded(home);
    }
  }

  function closeOnEscape(e: KeyboardEvent) {
    if (e.key === "Escape") {
      onclose();
    }
  }
</script>

<svelte:window onkeydown={closeOnEscape} />

<!-- The keyboard's way out is Esc, on the window above. This is the mouse's. It goes by the press, because a
     drag that starts in the text box and ends out here also counts as a click out here. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" data-testid="add-repository-scrim" onmousedown={onclose}>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label="Add repository"
    tabindex="-1"
    onmousedown={(e) => {
      e.stopPropagation();
    }}
  >
    <form onsubmit={submit}>
      <label>
        Repository
        <input
          {@attach takeFocus}
          bind:value={adding.input}
          readonly={step.step === "adding"}
          placeholder="owner/name"
          autocomplete="off"
          spellcheck="false"
        />
      </label>
      {#if step.step === "checking"}
        <p>Asking GitHub…</p>
      {:else if step.step === "failed"}
        <p role="alert">{step.problem}</p>
      {:else if step.step === "held"}
        <p>{`Tsugiki has ${step.name} already.`}</p>
      {:else if check}
        <h2>{check.repository.name}</h2>
        {#each describeCheck(check) as line (line)}
          <p>{line}</p>
        {/each}
        <MirrorChoices
          repository={check.repository.name}
          cost={check.mirror}
          choosing={adding.mirror}
          disabled={step.step === "adding"}
        />
      {/if}
      <div class="buttons">
        <button type="button" onclick={onclose}>Cancel</button>
        <button type="submit" disabled={waiting || undecided}>{action}</button>
      </div>
    </form>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 15%);
  }

  .dialog {
    width: min(640px, calc(100vw - 32px));
    margin: 60px auto 0;
    padding: 12px;
    box-sizing: border-box;
    font-size: 13px;
    background: #fff;
    border: 1px solid #ccc;
    border-radius: 6px;
    box-shadow: 0 8px 30px rgb(0 0 0 / 25%);
  }

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

  h2 {
    margin: 12px 0 0;
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
