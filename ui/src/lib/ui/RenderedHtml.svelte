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
  class="rendered reading"
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

  .rendered :global(p) {
    margin: 6px 0;
  }

  .rendered :global(a) {
    color: var(--accent);
  }

  .rendered :global(code) {
    padding: 1px 4px;
    font-size: 12.5px;
    background: var(--bg-chrome);
    border-radius: 4px;
  }

  .rendered :global(pre) {
    overflow-x: auto;
    padding: 8px;
    background: var(--bg-chrome);
    border-radius: 6px;
  }

  .rendered :global(pre code) {
    padding: 0;
    background: none;
  }

  .rendered :global(blockquote) {
    margin: 6px 0;
    padding-left: 10px;
    color: var(--fg-muted);
    border-left: 3px solid var(--fg-muted);
  }

  .rendered :global(img) {
    max-width: 100%;
  }

  /* GitHub's custom emoji come as images, and sit in the line at its height. */
  .rendered :global(img.emoji),
  .rendered :global(g-emoji img) {
    height: 1em;
    vertical-align: -0.1em;
  }

  .rendered :global(table) {
    border-collapse: collapse;
  }

  .rendered :global(td),
  .rendered :global(th) {
    padding: 2px 8px;
    border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
  }

  /* A suggested change, as GitHub renders one: the lines it replaces, then the lines it suggests. */
  .rendered :global(.js-suggested-changes-blob) {
    margin: 8px 0;
    border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
    border-radius: 6px;
    font-size: 12px;
  }

  .rendered :global(.js-suggested-changes-blob table) {
    width: 100%;
  }

  .rendered :global(.js-suggested-changes-blob td) {
    border: 0;
  }

  .rendered :global(.blob-num) {
    display: none;
  }

  .rendered :global(.blob-code-inner) {
    white-space: pre;
  }

  .rendered :global(.blob-code-deletion) {
    background: color-mix(in srgb, var(--del) 18%, transparent);
  }

  .rendered :global(.blob-code-addition) {
    background: color-mix(in srgb, var(--add) 18%, transparent);
  }

  .rendered :global(.blob-code-deletion .x) {
    background: color-mix(in srgb, var(--del) 35%, transparent);
  }

  .rendered :global(.blob-code-addition .x) {
    background: color-mix(in srgb, var(--add) 35%, transparent);
  }
</style>
