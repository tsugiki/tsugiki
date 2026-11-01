<script lang="ts">
  // PROTOTYPE — throwaway. Three whole-app layouts for gh-rev v1, switchable via ?variant=A|B|C and
  // the floating bar (Alt+← / Alt+→). All three read the same in-memory model built from a read-only
  // snapshot of python/cpython (src/lib/proto/fixture.json); nothing is ever written to GitHub.
  import { page } from "$app/state";
  import { goto } from "$app/navigation";
  import VariantA from "$lib/proto/VariantA.svelte";
  import VariantB from "$lib/proto/VariantB.svelte";
  import VariantC from "$lib/proto/VariantC.svelte";
  import Switcher from "$lib/proto/Switcher.svelte";
  import { toast } from "$lib/proto/model.svelte";

  const VARIANTS = [
    { key: "A", name: "Three panes (mail client)" },
    { key: "B", name: "Tree first, drill in" },
    { key: "C", name: "VS Code-like workspace" },
  ];
  const variant = $derived(page.url.searchParams.get("variant") ?? "A");
  // ?pr=N opens a PR straight away (handy for comparing the same PR across variants).
  const start = $derived(+(page.url.searchParams.get("pr") ?? 0) || null);
  const startIssue = $derived(+(page.url.searchParams.get("issue") ?? 0) || null);
  const pick = (k: string) => goto(`?variant=${k}${start ? `&pr=${start}` : ""}`, { replaceState: true, keepFocus: true, noScroll: true });

  let showToast = $state(false);
  $effect(() => {
    if (!toast.at) return;
    showToast = true;
    const t = setTimeout(() => (showToast = false), 3500);
    return () => clearTimeout(t);
  });
</script>

<svelte:head><title>gh-rev — app shell PROTOTYPE</title></svelte:head>

{#key variant}
  {#if variant === "B"}<VariantB {start} />{:else if variant === "C"}<VariantC {start} />{:else}<VariantA {start} {startIssue} />{/if}
{/key}
{#if showToast}<div class="toast">{toast.text}</div>{/if}
<Switcher variants={VARIANTS} current={variant} onPick={pick} />

<style>
  :global(:root) {
    --sans: system-ui, -apple-system, "Segoe UI", sans-serif;
    --mono: ui-monospace, "Cascadia Mono", "SF Mono", Consolas, monospace;
    --bg: #ffffff; --fg: #1f2328; --muted: #656d76; --line: #d8dee4; --panel: #f6f8fa;
    --hover: #eef1f4; --sel: #ddf4ff; --accent: #0969da; --on-accent: #fff;
    --good: #1a7f37; --bad: #cf222e; --warn: #9a6700; --merged: #8250df;
    --add-bg: #e6ffec; --del-bg: #ffebe9; --hunk-bg: #ddf4ff;
    --chrome: #2c2c2c; --chrome-fg: #fff;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme="light"])) {
      --bg: #0d1117; --fg: #e6edf3; --muted: #8d96a0; --line: #30363d; --panel: #161b22;
      --hover: #1c2129; --sel: #1f3a5f; --accent: #4493f8; --on-accent: #fff;
      --good: #3fb950; --bad: #f85149; --warn: #d29922; --merged: #ab7df8;
      --add-bg: #12261e; --del-bg: #25171c; --hunk-bg: #121d2f;
      --chrome: #181818; --chrome-fg: #fff;
    }
  }
  :global(html, body) { margin: 0; height: 100%; overflow: hidden; }
  :global(body) { background: var(--bg); color: var(--fg); font: 14px var(--sans); }
  :global(button) { font: inherit; color: inherit; background: var(--panel); border: 1px solid var(--line); border-radius: 4px; padding: 2px 8px; cursor: pointer; }
  :global(button:hover) { background: var(--hover); }
  :global(button:disabled) { opacity: 0.5; cursor: default; }
  :global(.md p) { margin: 6px 0; }
  :global(.md code) { font: 12.5px var(--mono); background: var(--panel); padding: 1px 4px; border-radius: 4px; }
  :global(.md pre) { background: var(--panel); padding: 8px; border-radius: 6px; overflow: auto; }
  :global(.md pre code) { background: none; padding: 0; }
  :global(.md blockquote) { margin: 6px 0; padding-left: 10px; border-left: 3px solid var(--line); color: var(--muted); }
  :global(.md a) { color: var(--accent); }
  :global(.md img) { max-width: 100%; }
  :global(.md g-emoji img) { height: 1em; }
  :global(.hljs-keyword, .hljs-meta .hljs-keyword) { color: #cf222e; }
  :global(.hljs-string) { color: #0a3069; }
  :global(.hljs-comment) { color: #6e7781; font-style: italic; }
  :global(.hljs-number, .hljs-literal) { color: #0550ae; }
  :global(.hljs-title, .hljs-title.function_) { color: #8250df; }
  :global(.hljs-type, .hljs-built_in) { color: #953800; }
  :global(.hljs-meta) { color: #116329; }
  @media (prefers-color-scheme: dark) {
    :global(.hljs-keyword) { color: #ff7b72; } :global(.hljs-string) { color: #a5d6ff; }
    :global(.hljs-comment) { color: #8b949e; } :global(.hljs-number, .hljs-literal) { color: #79c0ff; }
    :global(.hljs-title, .hljs-title.function_) { color: #d2a8ff; } :global(.hljs-type, .hljs-built_in) { color: #ffa657; }
    :global(.hljs-meta) { color: #7ee787; }
  }
  .toast { position: fixed; bottom: 56px; left: 50%; transform: translateX(-50%); background: var(--fg); color: var(--bg); padding: 8px 14px; border-radius: 6px; font-size: 13px; z-index: 90; max-width: 70vw; }
</style>
