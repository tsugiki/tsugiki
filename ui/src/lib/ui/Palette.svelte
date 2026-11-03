<script lang="ts">
  // The command palette's shell: it opens, takes the focus and closes. Commands come with what they command.
  let { onclose }: { onclose: () => void } = $props();

  function takeFocus(input: HTMLInputElement) {
    const previous = document.activeElement;
    input.focus();
    return () => {
      if (previous instanceof HTMLElement) {
        previous.focus();
      }
    };
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
<div class="scrim" data-testid="palette-scrim" onmousedown={onclose}>
  <div
    class="palette"
    role="dialog"
    aria-modal="true"
    aria-label="Command palette"
    tabindex="-1"
    onmousedown={(e) => {
      e.stopPropagation();
    }}
  >
    <input
      {@attach takeFocus}
      role="combobox"
      aria-expanded="false"
      aria-controls="palette-commands"
      aria-label="Command"
      placeholder="Type a command"
    />
    <p id="palette-commands">No commands yet</p>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 15%);
  }

  .palette {
    width: min(640px, calc(100vw - 32px));
    margin: 60px auto 0;
    background: #fff;
    border: 1px solid #ccc;
    border-radius: 6px;
    box-shadow: 0 8px 30px rgb(0 0 0 / 25%);
  }

  input {
    width: 100%;
    box-sizing: border-box;
    padding: 8px 10px;
    font-size: 14px;
    border: 0;
    border-bottom: 1px solid #ccc;
    border-radius: 6px 6px 0 0;
  }

  p {
    margin: 0;
    padding: 8px 10px;
    font-size: 13px;
    color: #555;
  }
</style>
