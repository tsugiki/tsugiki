<!-- PROTOTYPE, throwaway (#365): wraps the Files tab's diff. F12 and Ctrl+click go to the definition of the
     symbol at the caret, Alt+F12 peeks at it; the variant decides where the peek shows. A hit in a file the
     PR changes, on a line the diff shows, scrolls the diff there; any other opens the file view. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import { commands, type ProtoHit } from "$lib/data/bindings";
  import type { PrFilesState } from "$lib/state/pr-files.svelte";
  import { currentOs, keyName } from "../keys";
  import { proto, symbolAtCaret, type Lookup } from "./definition-peek.svelte";
  import FileView from "./FileView.svelte";
  import PeekBody from "./PeekBody.svelte";
  import PeekSwitcher from "./PeekSwitcher.svelte";

  let { files, children }: { files: PrFilesState; children: Snippet } = $props();

  const os = currentOs();
  let host = $state<HTMLElement>();
  let point: { x: number; y: number } | undefined;
  const ready = $derived(files.view?.state === "ready" ? files.view : undefined);

  $effect(() => () => {
    proto.reset();
  });

  /** The file of the diff's row `index`: the last whose header comes at or before it. */
  const fileOfRow = (index: number) => {
    let found = -1;
    for (const [at, file] of (ready?.files ?? []).entries()) if (file.row <= index) found = at;
    return found;
  };

  /** The symbol's line in the diff, and the commit to look it up at. */
  function fromDiff(element: Element, word: string): { lookup: Lookup; place: number } | undefined {
    const view = ready;
    const rowElement = element.closest("[data-place]");
    if (!view || !(rowElement instanceof HTMLElement)) return undefined;
    const place = Number(rowElement.dataset["place"]);
    const at = files.layout.at(place);
    const base = { repository: files.repository, number: files.number, word };
    if (at.kind === "line") {
      const file = view.files[at.file];
      if (!file) return undefined;
      return { place, lookup: { ...base, commit: view.head, which: "head", from: file.path, fromLine: at.new } };
    }
    let index: number | null = null;
    if (at.kind === "row") index = at.row;
    else if (at.kind === "pair")
      index = element.closest(".side")?.classList.contains("removed") ? at.removed : (at.added ?? at.removed);
    if (index === null) return undefined;
    const row = files.row(index);
    const file = view.files[fileOfRow(index)];
    if (!row || !file) return undefined;
    if (row.kind === "removed")
      return {
        place,
        lookup: { ...base, commit: view.base, which: "base", from: file.old_path ?? file.path, fromLine: row.old },
      };
    if (row.kind === "added" || row.kind === "context")
      return { place, lookup: { ...base, commit: view.head, which: "head", from: file.path, fromLine: row.new } };
    return undefined;
  }

  /** The symbol's line in a peek or the file view, which share their commit. */
  function fromCode(element: Element, word: string): Lookup | undefined {
    const line = Number((element.closest("[data-line]") as HTMLElement | null)?.dataset["line"]);
    if (element.closest("[data-proto-file]") && proto.fileView) {
      const view = proto.fileView;
      return { ...view.lookup, word, from: view.path, fromLine: line };
    }
    const hit = proto.selectedHit();
    if (element.closest("[data-proto-peek]") && proto.peek && hit)
      return { ...proto.peek.lookup, word, from: hit.path, fromLine: line };
    return undefined;
  }

  type Found = { lookup: Lookup; place: number; rect: DOMRect; inPeek: boolean };

  function symbolIn(region: HTMLElement, at: { x: number; y: number } | undefined): Found | undefined {
    const symbol = symbolAtCaret(region, at);
    if (!symbol) return undefined;
    if (region.closest("[data-proto-peek], [data-proto-file]")) {
      const lookup = fromCode(symbol.element, symbol.word);
      const inPeek = region.closest("[data-proto-peek]") !== null;
      return lookup && { lookup, place: inPeek ? (proto.peek?.place ?? -1) : -1, rect: symbol.rect, inPeek };
    }
    const found = fromDiff(symbol.element, symbol.word);
    return found && { ...found, rect: symbol.rect, inPeek: false };
  }

  /** Opens `hit` in full: in the diff when it shows that line, else in the file view. */
  async function openHit(lookup: Lookup, hit: ProtoHit) {
    const view = ready;
    if (!view) return;
    const at = view.files.findIndex((file) =>
      lookup.which === "head" ? file.path === hit.path : (file.old_path ?? file.path) === hit.path,
    );
    const file = view.files[at];
    if (file) {
      const end = view.files[at + 1]?.row ?? view.rows;
      const chunk = { repository: files.repository, number: files.number, head: view.head, base: view.base };
      const rows = await commands.pullRequestRows({ ...chunk, start: file.row, end });
      if (rows.status === "ok") {
        const into = rows.data.findIndex((row) =>
          lookup.which === "head"
            ? (row.kind === "added" || row.kind === "context") && row.new === hit.line
            : (row.kind === "removed" || row.kind === "context") && row.old === hit.line,
        );
        if (into >= 0) {
          proto.closePeek();
          proto.fileView = undefined;
          files.jump = { row: file.row + into };
          proto.flashRow(file.row + into);
          host?.querySelector<HTMLElement>(".scroller")?.focus();
          return;
        }
      }
    }
    proto.openFile(lookup, hit, file !== undefined);
  }
  proto.opener = (lookup, hit) => void openHit(lookup, hit);

  async function goTo(found: Found) {
    proto.say(`Finding the definition of ${found.lookup.word}…`, 0);
    const answer = await proto.find(found.lookup);
    if (answer.state === "cloning") {
      proto.say(`Go to definition waits for the mirror's clone: ${String(proto.cloned)}%, about 3 minutes left.`, 5000);
    } else if (answer.state === "error") {
      proto.say(answer.error, 5000);
    } else if (answer.state === "done") {
      const hits = answer.hits.hits;
      if (hits.length === 0) {
        proto.say(`No definition of ${found.lookup.word} at ${found.lookup.commit.slice(0, 7)}.`, 4000);
      } else if (hits.length === 1 && hits[0]) {
        proto.say(answer.hits.fallback ? `No definition found: the only whole-word match.` : undefined, 3000);
        void openHit(found.lookup, hits[0]);
      } else {
        proto.say(undefined);
        // Several: the peek lists them, as VS Code does.
        proto.peek = { lookup: found.lookup, place: found.place, rect: found.rect, answer, selected: 0, trail: [] };
        proto.select(0);
      }
    }
  }

  function peekAt(found: Found) {
    const trail = found.inPeek && proto.peek ? [...proto.peek.trail, proto.peek.lookup] : [];
    void proto.openPeek(found.lookup, found.place, found.rect, trail);
  }

  const stop = (e: Event) => {
    e.preventDefault();
    e.stopImmediatePropagation();
  };

  function onkey(e: KeyboardEvent) {
    if (!host) return;
    const key = keyName(e, os);
    const target = e.target instanceof HTMLElement ? e.target : null;
    const inHost = target !== null && host.contains(target);
    const inHits = target?.closest("[data-proto-hits]") != null;
    const inLayer = target?.closest("[data-proto-peek], [data-proto-file]") != null;
    if ((key === "f12" || key === "alt+f12") && inHost && target) {
      stop(e);
      if (inHits && key === "f12") {
        proto.openSelected();
        return;
      }
      const found = symbolIn(target, point);
      if (!found) {
        proto.say("Click a symbol first, so the caret is on it.");
        return;
      }
      if (key === "f12") void goTo(found);
      else peekAt(found);
    } else if (key === "escape" && proto.peek) {
      stop(e);
      proto.closePeek();
      host.querySelector<HTMLElement>(proto.fileView ? "[data-proto-file] [data-proto-code]" : ".scroller")?.focus();
    } else if (key === "escape" && proto.fileView) {
      stop(e);
      proto.back();
    } else if (key === "alt+arrowleft" && proto.fileView) {
      stop(e);
      proto.back();
      if (!proto.fileView) host.querySelector<HTMLElement>(".scroller")?.focus();
    } else if (key === "alt+arrowright" && proto.ahead.length > 0) {
      stop(e);
      proto.forward();
    } else if (inHits && (key === "arrowdown" || key === "arrowup")) {
      stop(e);
      proto.select((proto.peek?.selected ?? 0) + (key === "arrowdown" ? 1 : -1));
    } else if (inHits && key === "enter") {
      stop(e);
      proto.openSelected();
    } else if (inLayer && (key === "arrowdown" || key === "arrowup" || key === "pagedown" || key === "pageup")) {
      // The peek's and the file view's code scroll themselves, so the Issue list's cursor stays.
      e.stopImmediatePropagation();
    }
  }

  $effect(() => {
    window.addEventListener("keydown", onkey, { capture: true });
    return () => {
      window.removeEventListener("keydown", onkey, { capture: true });
    };
  });

  const cardStyle = $derived.by(() => {
    const rect = proto.peek?.rect;
    if (!rect) return "";
    const width = Math.min(760, innerWidth - 16);
    const height = 340;
    const left = Math.max(8, Math.min(rect.left - 40, innerWidth - width - 8));
    const below = rect.bottom + 4 + height < innerHeight - 30;
    const top = below ? rect.bottom + 4 : Math.max(8, rect.top - height - 4);
    return `left:${String(left)}px;top:${String(top)}px;width:${String(width)}px;height:${String(height)}px`;
  });
  /** Variant A shows inline only under a diff row; from the file view it falls back to the split. */
  const split = $derived(
    proto.peek !== undefined && (proto.variant === "B" || (proto.variant === "A" && proto.peek.place < 0)),
  );
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="host"
  bind:this={host}
  onpointerdown={(e) => {
    point = { x: e.clientX, y: e.clientY };
  }}
  onclick={(e) => {
    if (!e.ctrlKey || !(e.target instanceof Element) || !e.target.closest(".text")) return;
    const region = e.target.closest<HTMLElement>("[data-proto-code], .scroller");
    if (!region) return;
    getSelection()?.removeAllRanges();
    const found = symbolIn(region, { x: e.clientX, y: e.clientY });
    if (found) {
      e.preventDefault();
      void goTo(found);
    }
  }}
>
  <div class="diff-area">
    {@render children()}
    {#if proto.fileView}
      <FileView view={proto.fileView} depth={proto.stack.length} />
    {/if}
    {#if proto.notice}
      <div class="notice" role="status">{proto.notice}</div>
    {/if}
  </div>
  {#if split && proto.peek}
    <div class="split">
      <PeekBody peek={proto.peek} arrangement="left" />
    </div>
  {/if}
  {#if proto.variant === "C" && proto.peek}
    <div class="card" style={cardStyle}>
      <PeekBody peek={proto.peek} arrangement="top" />
    </div>
  {/if}
  <PeekSwitcher />
</div>

<style>
  .host {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }

  .diff-area {
    position: relative;
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }

  .split {
    position: relative;
    flex: 0 0 42%;
    min-height: 160px;
    border-top: 2px solid #0969da;
  }

  .card {
    position: fixed;
    z-index: 50;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
    border-radius: 6px;
    background: Canvas;
    box-shadow: 0 8px 24px rgb(0 0 0 / 25%);
  }

  .notice {
    position: absolute;
    top: 6px;
    right: 16px;
    z-index: 10;
    max-width: 60%;
    padding: 3px 10px;
    border-radius: 4px;
    font-family: system-ui, sans-serif;
    font-size: 12px;
    color: #fff;
    background: rgb(36 41 47 / 90%);
  }
</style>
