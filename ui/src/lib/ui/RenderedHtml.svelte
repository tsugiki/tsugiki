<script lang="ts">
  // HTML written by strangers, as GitHub rendered it. The content security policy stops anything in it from
  // running or loading (ADR 0005), and this keeps it from taking the window away from Tsugiki.
  let { html, onlink }: { html: string; onlink: (address: string) => void } = $props();

  /**
   * Shows `html` in `node` without a refresh or a base address, either of which could move the window if
   * GitHub's sanitizer let it through. A template's contents are inert, so parsing them there acts on
   * nothing, and the parsed nodes move into the page as they are: parsing a second time could make markup
   * this never saw.
   */
  function show(node: HTMLElement) {
    const template = document.createElement("template");
    template.innerHTML = html;
    for (const tag of template.content.querySelectorAll("meta, base")) tag.remove();
    node.replaceChildren(template.content);
  }

  /** Follows a link in the system browser. Enter on a link arrives as a click, so keys need nothing more. */
  function followLink(e: MouseEvent) {
    // Only an element takes a click.
    const link = (e.target as Element).closest("a, area");
    if (!link) return;
    e.preventDefault();
    const address = link.getAttribute("href");
    // A link within the text moves to a heading in a page Tsugiki doesn't show whole.
    if (address && !address.startsWith("#")) onlink(address);
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="rendered"
  {@attach show}
  onclick={followLink}
  onauxclick={(e) => {
    // A right click opens a menu, and the side buttons go back and forward.
    if (e.button === 1) followLink(e);
  }}
  onsubmit={(e) => {
    e.preventDefault();
  }}
></div>

<style>
  .rendered {
    overflow-wrap: anywhere;
  }

  .rendered :global(pre) {
    overflow-x: auto;
    padding: 8px;
    background: color-mix(in srgb, currentColor 8%, transparent);
  }

  .rendered :global(code) {
    font-family: ui-monospace, Consolas, monospace;
  }

  .rendered :global(blockquote) {
    margin: 0;
    padding-left: 12px;
    border-left: 3px solid color-mix(in srgb, currentColor 25%, transparent);
  }

  .rendered :global(img) {
    max-width: 100%;
  }

  .rendered :global(table) {
    border-collapse: collapse;
  }

  .rendered :global(td),
  .rendered :global(th) {
    padding: 2px 8px;
    border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
  }
</style>
