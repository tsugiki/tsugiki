<script lang="ts">
  // PROTOTYPE — throwaway. One open Repository: three panes, like a mail client (layout A).
  // Sidebar: the Repository switcher, then where to look (Workbench, Built-in views, All issues,
  // Filters, Pinned, Archived, Muted). Issue list: one item per Issue, Standalone PR or Dependency updates, like
  // mail. Detail pane: the selected item. Alt+← / Alt+→ go back and forward; Esc from a PR to its Issue.
  import type { Scope, WorkItem } from "$lib/core/types";
  import { VIEWS } from "$lib/core/workbench";
  import { issueLabel } from "$lib/core/conventions";
  import { linkTarget } from "$lib/core/links";
  import { wb, my, app, REPOS, boardOf, openRepo, openPr, openIssue, togglePin, toggleMark, say, archive, mute, muteAuthor, unhide, unmuteAuthor } from "$lib/state/app.svelte";
  import { openInBrowser } from "$lib/platform/shell";
  import { settingsUi, openSettings, value, setValue } from "$lib/state/settings.svelte";
  import { ago, waitingText, depsUrl } from "./format";
  import StateDot from "./StateDot.svelte";
  import KindIcon from "./KindIcon.svelte";
  import PrStatus from "./PrStatus.svelte";
  import IssueView from "./IssueView.svelte";
  import DepsView from "./DepsView.svelte";
  import DiffView from "./DiffView.svelte";
  import Conversation from "./Conversation.svelte";
  import ReviewBar from "./ReviewBar.svelte";
  import Palette from "./Palette.svelte";
  import Peek from "./Peek.svelte";
  import Help from "./Help.svelte";
  import { keymap, type Bindings } from "./keys";

  let { start = null, startIssue = null, startAuthor = null, startView = null, startPeek = null }: {
    start?: number | null; startIssue?: number | null; startAuthor?: string | null; startView?: string | null; startPeek?: string | null;
  } = $props();
  // svelte-ignore state_referenced_locally
  let scope = $state<Scope>(startAuthor ? { kind: "author", login: startAuthor } : startView === "issues" ? { kind: "issues", q: "" } : startView === "archived" ? { kind: "archived" } : startView === "muted" ? { kind: "muted" } : { kind: "all" });
  // Selection: an Issue "i:N", a PR "p:N", or "deps". History for Alt+← / Alt+→.
  // svelte-ignore state_referenced_locally
  let sel = $state<string | null>(app.select ?? (start ? `p:${start}` : startIssue ? `i:${startIssue}` : startView === "deps" ? "deps" : null));
  let history = $state<string[]>([]);
  let hpos = $state(-1);
  let tab = $state<"conv" | "files">("conv");
  let side = $state(true);
  let split = $state(value("review.diff_view") === "split");
  let showQuiet = $state(false);
  let palette = $state<string | null>(null);
  let help = $state(false);
  // svelte-ignore state_referenced_locally
  const pk = startPeek?.match(/^(.+)\/(\d+)$/);
  let peek = $state<{ repo: string; number: number; added: boolean; url: string } | null>(
    pk ? { repo: pk[1], number: +pk[2], added: REPOS.includes(pk[1]), url: `https://github.com/${pk[1]}/issues/${pk[2]}` } : null);
  let diff = $state<DiffView>();
  let sortBy = $state<"attention" | "updated">("attention");
  // All issues' quick filters. At least one stays on.
  let show = $state({ issue: true, pr: true });
  function toggleShow(k: "issue" | "pr") {
    const other = k === "issue" ? "pr" : "issue";
    show = show[k] && !show[other] ? { issue: !show.issue, pr: !show.pr } : { ...show, [k]: !show[k] };
  }

  const repo = $derived(wb.repo);
  const board = $derived(wb.workbench(scope));
  const everything = $derived(wb.workbench({ kind: "all" }));
  const selKind = $derived(sel?.slice(0, 1));
  const selNum = $derived(sel && sel !== "deps" ? +sel.slice(2) : 0);
  const current = $derived(selKind === "p" ? wb.pr(selNum) : null);
  // Sidebar counts: what the user hid. Muted counts Muted authors too, since they are listed there.
  const archivedCount = $derived(everything.hidden.filter((i) => i.hidden === "archived").length);
  const mutedCount = $derived(everything.hidden.filter((i) => i.hidden === "muted").length + my.mutedAuthors.size);
  const depsItem = $derived([...everything.active, ...everything.quiet, ...everything.hidden].find((i) => i.kind === "deps") ?? null);
  /** The list item the selection belongs to. */
  // All issues: what changed since the last sync first, then the rest loaded so far.
  const lastSync = $derived(new Date(new Date(wb.takenAt).getTime() - 86400e3).toISOString());
  const byUpdated = (a: WorkItem, b: WorkItem) => b.updatedAt.localeCompare(a.updatedAt);
  const kindCount = $derived(scope.kind === "issues" ? { issue: board.active.filter((i) => i.kind === "issue").length, pr: board.active.filter((i) => i.kind === "pr").length } : null);
  const visible = $derived.by(() => {
    if (scope.kind === "issues") return board.active.filter((i) => show[i.kind as "issue" | "pr"]);
    const active = sortBy === "updated" ? [...board.active].sort(byUpdated) : board.active;
    return [...active, ...(showQuiet ? board.quiet : [])];
  });
  const flat = $derived(visible.map((i) => i.key));
  const selItem = $derived(
    selKind === "i" || sel === "deps" ? sel :
    // In All issues an open PR is its own row.
    current && flat.includes(`p:${current.number}`) ? `p:${current.number}` :
    current ? (current.issue ? `i:${current.issue}` : current.dep && repo.maintained && current.state === "OPEN" ? "deps" : `p:${current.number}`) : null);
  const label = (it: WorkItem) => (it.kind === "issue" ? issueLabel(repo, it.number!) : it.kind === "pr" ? `#${it.number}` : "⇡");
  const scopeName = $derived(
    scope.kind === "all" ? "Workbench" : scope.kind === "pinned" ? "Pinned issues" : scope.kind === "filter" ? scope.name :
    scope.kind === "author" ? `author:${scope.login}` : scope.kind === "issues" ? "All issues" : scope.kind === "archived" ? "Archived" : scope.kind === "muted" ? "Muted" :
    VIEWS.find((v) => v.key === (scope as { view: string }).view)!.name);

  function select(key: string, push = true) {
    sel = key;
    if (push) { history = [...history.slice(0, hpos + 1), key]; hpos = history.length - 1; }
    if (key.startsWith("p:")) openPr(+key.slice(2)); else if (key.startsWith("i:")) openIssue(+key.slice(2));
    queueMicrotask(() => document.querySelector(`[data-key="${selItem}"]`)?.scrollIntoView({ block: "nearest" }));
  }
  // The status bar (a Pending review) asks for an item in this Repository.
  $effect(() => { if (app.goto) { const k = app.goto; app.goto = null; select(k); } });
  function go(d: 1 | -1) {
    const k = history[hpos + d];
    if (k) { hpos += d; select(k, false); }
  }
  function move(d: number) {
    const i = flat.indexOf(selItem ?? "");
    const k = flat[Math.max(0, Math.min(flat.length - 1, i + d))];
    if (k) select(k);
  }
  /** Clicking a person opens a temporary `author:<login>` Filter in the Issue list. */
  function showAuthor(login: string) { scope = { kind: "author", login }; }
  /** Esc in a PR view goes back to its Issue view. */
  function back() { if (current) select(current.issue ? `i:${current.issue}` : selItem ?? `p:${current.number}`); }
  const scopeEq = (s: Scope) => JSON.stringify(s) === JSON.stringify(scope);
  const itemLabel = (key: string) => (key === "deps" ? "Dependency updates" : key.startsWith("i:") ? issueLabel(repo, +key.slice(2)) : `#${key.slice(2)}`);

  function githubUrl(): string | null {
    if (current) return current.url;
    if (selKind === "i") return wb.issue(selNum)?.url ?? `https://github.com/${repo.name}/issues/${selNum}`;
    if (sel === "deps") return depsUrl(repo.name, depsItem?.author ?? null);
    return null;
  }
  function openOnGithub() { const u = githubUrl(); if (u) openInBrowser(u); }

  /**
   * Links in descriptions and comments: never navigate gh-rev's own window. Here → the Detail pane;
   * elsewhere on GitHub → Peek; anything else, or Ctrl+click → the system browser.
   */
  function onLink(e: MouseEvent) {
    const a = (e.target as HTMLElement).closest("a");
    const href = a?.getAttribute("href");
    if (!a || !href) return;
    e.preventDefault();
    if (href.startsWith("#")) return;
    const url = a.href;
    if (e.ctrlKey || e.metaKey || a.dataset.web != null) return openInBrowser(url);
    const t = linkTarget(url, repo.name, REPOS);
    if (t.kind === "here") {
      if (wb.raw(t.number)) select(`p:${t.number}`);
      else if (wb.issue(t.number)) select(`i:${t.number}`);
      else peek = { repo: repo.name, number: t.number, added: false, url };
    } else if (t.kind === "peek") peek = { repo: t.repo, number: t.number, added: t.added, url };
    else openInBrowser(url);
  }
  /** Peek's "Open in gh-rev": switch Repository with the item selected there. */
  function openElsewhere(r: string, n: number) {
    const name = REPOS.find((x) => x.toLowerCase() === r.toLowerCase());
    if (name) openRepo(name, boardOf(name).raw(n) ? `p:${n}` : `i:${n}`);
  }

  const hideSel = (how: "archive" | "mute") => { if (selItem) (how === "archive" ? archive : mute)(selItem, itemLabel(selItem)); };
  const commands = $derived([
    ...REPOS.map((r) => ({ label: `Repository: ${r}`, key: r === repo.name ? "current" : undefined, run: () => openRepo(r) })),
    { label: "Home: all Repositories", run: () => openRepo(null) },
    { label: "Preferences: Open Settings", key: "Ctrl+,", run: () => openSettings() },
    { label: "View: Toggle Status Bar", run: () => setValue("status_bar.visible", !value("status_bar.visible")) },
    { label: "View: Toggle Key Hints under the Issue List", run: () => setValue("appearance.list_key_hints", !value("appearance.list_key_hints")) },
    { label: "Preferences: Keyboard Shortcuts", run: () => openSettings("Keybindings") },
    { label: `Preferences: Settings for ${repo.name}`, run: () => openSettings(`repo:${repo.name}`) },
    { label: "Toggle sidebar", key: "Ctrl+B", run: () => (side = !side) },
    { label: "Unified ↔ side-by-side", key: "Ctrl+Alt+U", run: () => (split = !split) },
    { label: "Open on GitHub", key: "Ctrl+Shift+G", run: openOnGithub },
    { label: "Archive selected item", key: "E", run: () => hideSel("archive") },
    { label: "Mute selected item", key: "M", run: () => hideSel("mute") },
    ...(current && current.author !== wb.me ? [{ label: `Mute author ${current.author}`, run: () => muteAuthor(current!.author) }] : []),
    { label: "Local mark on selected PR", key: "Ctrl+K M", run: () => current && toggleMark(current.number) },
    { label: "Pin or unpin Issue", key: "Ctrl+K P", run: () => selKind === "i" && togglePin(selNum) },
    { label: "Show quiet items", run: () => (showQuiet = !showQuiet) },
    { label: "All issues", run: () => (scope = { kind: "issues", q: "" }) },
    { label: "All issues: show Issues only", run: () => { scope = { kind: "issues", q: "" }; show = { issue: true, pr: false }; } },
    { label: "All issues: show PRs only", run: () => { scope = { kind: "issues", q: "" }; show = { issue: false, pr: true }; } },
    { label: "View: Archived", run: () => (scope = { kind: "archived" }) },
    { label: "View: Muted", run: () => (scope = { kind: "muted" }) },
    ...VIEWS.map((v) => ({ label: `View: ${v.name}`, run: () => (scope = { kind: "view", view: v.key }) })),
    ...wb.filters.map((f) => ({ label: `Filter: ${f.name}`, run: () => (scope = { kind: "filter", name: f.name }) })),
  ]);
  // While Settings is open, it owns the keyboard.
  const onKey = keymap(() => settingsUi.open ? ({} as Bindings) : ({
    "ctrl+,": () => openSettings(),
    "ctrl+shift+p": () => (palette = ">"),
    "ctrl+p": () => (palette = ""),
    "ctrl+r": () => (palette = ">Repository: "),
    "?": () => (help = !help),
    escape: () => { if (palette != null || help) { palette = null; help = false; } else if (!peek) back(); },
    arrowdown: () => move(1),
    arrowup: () => move(-1),
    "alt+arrowleft": () => go(-1),
    "alt+arrowright": () => go(1),
    enter: () => current && (tab = tab === "conv" ? "files" : "conv"),
    e: () => hideSel("archive"),
    m: () => hideSel("mute"),
    "ctrl+shift+g": openOnGithub,
    "ctrl+b": () => (side = !side),
    "ctrl+alt+u": () => (split = !split),
    f7: () => { tab = "files"; diff?.jump("change", 1); },
    "shift+f7": () => { tab = "files"; diff?.jump("change", -1); },
    f8: () => { tab = "files"; diff?.jump("thread", 1); },
    "shift+f8": () => { tab = "files"; diff?.jump("thread", -1); },
    f12: () => say("Go to definition: git grep at the head commit (stub in this prototype)"),
    "alt+f12": () => say("Peek definition (stub in this prototype)"),
    "ctrl+shift+e": () => say(`Would run: code -g <Review checkout>/… for #${current?.number}`),
    "ctrl+k m": () => current && toggleMark(current.number),
    "ctrl+k p": () => selKind === "i" && togglePin(selNum),
  }));
</script>

<svelte:window onkeydown={onKey} />

<div class="a" class:noside={!side}>
  {#if side}
    <nav class="left">
      <div class="repohead">
        <button class="repo" onclick={() => (palette = ">Repository: ")} title="Switch Repository (Ctrl+R)"><span class="rn">{repo.name}</span> ▾</button>
        <button class="home" onclick={() => openSettings()} title="Settings (Ctrl+,)">⚙</button>
        <button class="home" onclick={() => openRepo(null)} title="Home: all Repositories">⌂</button>
      </div>
      {#if repo.maintained}<div class="role">maintained by you</div>{/if}
      {#snippet count(n: number)}{#if n}<span class="n">{n}</span>{/if}{/snippet}
      <button class="nav" class:on={scopeEq({ kind: "all" })} onclick={() => (scope = { kind: "all" })}>Workbench{@render count(wb.attentionCount({ kind: "all" }))}</button>
      <div class="sec">Built-in views</div>
      {#each VIEWS as v}
        <button class="nav" class:on={scopeEq({ kind: "view", view: v.key })} onclick={() => (scope = { kind: "view", view: v.key })}>{v.name}{@render count(wb.viewCount(v.key))}</button>
      {/each}
      <button class="nav" class:on={scope.kind === "archived"} onclick={() => (scope = { kind: "archived" })} title="Archived items: each comes back when something new needs you">Archived{@render count(archivedCount)}</button>
      <button class="nav" class:on={scope.kind === "muted"} onclick={() => (scope = { kind: "muted" })} title="Muted items and Muted authors: they stay hidden until you unmute them">Muted{@render count(mutedCount)}</button>
      <div class="sec">Filters</div>
      <button class="nav" class:on={scope.kind === "issues"} onclick={() => (scope = { kind: "issues", q: "" })} title="Every open Issue and PR loaded so far; type to search">All issues<span class="n">{wb.allIssueCount}{repo.small ? "" : "+"}</span></button>
      {#each wb.filters as f}
        <button class="nav" title={f.q} class:on={scopeEq({ kind: "filter", name: f.name })} onclick={() => (scope = { kind: "filter", name: f.name })}>{f.name}{@render count(wb.attentionCount({ kind: "filter", name: f.name }))}</button>
      {/each}
      {#if scope.kind === "author"}
        {@const login = scope.login}
        <div class="nav on tmp"><span>author:{login} <i class="muted">unsaved</i></span>
          <span class="tmpact"><button onclick={() => say(`Would save "author:${login}" as a Filter in your config file (prototype)`)} title="Save as Filter">Save</button><button onclick={() => (scope = { kind: "all" })} title="Close">✕</button></span></div>
      {/if}
      <div class="sec">Pinned</div>
      <button class="nav" class:on={scopeEq({ kind: "pinned" })} onclick={() => (scope = { kind: "pinned" })}>Pinned issues{@render count(wb.attentionCount({ kind: "pinned" }))}</button>
      <div class="hint">Numbers: items that need attention (PRs, for the Built-in views).</div>
    </nav>
  {/if}

  <section class="mid">
    <header class="lhead">
      <b>{scopeName}</b>
      <span class="grow"></span>
      {#if scope.kind !== "issues" && scope.kind !== "archived" && scope.kind !== "muted"}<button class="sort" onclick={() => (sortBy = sortBy === "attention" ? "updated" : "attention")} title="Change sort order">By {sortBy} ⇅</button>{/if}
    </header>
    {#if scope.kind === "issues"}
      {@const q = scope.q}
      <div class="search">
        <input value={q} oninput={(e) => (scope = { kind: "issues", q: e.currentTarget.value })} placeholder="Search: words, label:x, author:x, #123" />
        <button disabled={!q.trim()} onclick={() => say(`Would save "is:open ${show.issue && show.pr ? "" : show.issue ? "is:issue " : "is:pr "}${q}" as a Filter (prototype)`)}>Save</button>
      </div>
      <div class="quick">
        <button class="chip" class:on={show.issue} aria-pressed={show.issue} onclick={() => toggleShow("issue")} title="Show Issues"><KindIcon kind="issue" state="OPEN" /> Issues <span class="n">{kindCount?.issue}</span></button>
        <button class="chip" class:on={show.pr} aria-pressed={show.pr} onclick={() => toggleShow("pr")} title="Show PRs"><KindIcon kind="pr" state="OPEN" /> PRs <span class="n">{kindCount?.pr}</span></button>
      </div>
    {/if}
    <div class="list">
      {#if scope.kind === "muted" && my.mutedAuthors.size}
        <div class="group">Muted authors in {repo.name}</div>
        {#each [...my.mutedAuthors] as a}<div class="mutedrow"><span>{a}</span><button onclick={() => unmuteAuthor(a)}>Unmute</button></div>{/each}
      {/if}
      {#each visible as it, k (it.key)}
        {#if scope.kind === "issues" && (k === 0 || (visible[k - 1].updatedAt > lastSync) !== (it.updatedAt > lastSync))}
          <div class="group">{it.updatedAt > lastSync ? "Updated since the last sync" : "Earlier"}</div>
        {:else if scope.kind === "muted" && (k === 0 || visible[k - 1].hidden !== it.hidden)}
          <div class="group">{it.hidden === "muted" ? "Muted items" : "Hidden by a Muted author"}</div>
        {/if}
        {@const why = it.waiting.map(waitingText)}
        <div class="issue" class:att={it.attention > 0} class:deps={it.kind === "deps"}>
          <div class="item" class:sel={selItem === it.key}>
            <button class="ibody" data-key={it.key} onclick={() => select(it.key)} title={it.reasons.join(" · ")}>
              <span class="l1">{it.people.join("; ") || "—"}</span>
              <span class="l2"><span class="ititle">{#if it.kind === "issue"}<KindIcon kind="issue" state={it.issue?.state ?? "OPEN"} /> {:else if it.kind === "pr"}<KindIcon kind="pr" state={it.prs[0].state} draft={it.prs[0].isDraft} /> {/if}<span class="inum">{label(it)}</span> {it.title}</span>{#if it.kind !== "pr"}<span class="cnt">({it.total})</span>{/if}<span class="time">{ago(it.updatedAt)}</span></span>
              <span class="l3">
                {#if it.hidden}<i>{it.hidden === "archived" ? "archived: comes back when something new needs you" : it.hidden === "muted" ? "muted until you unmute it" : `author ${it.author} is muted`}</i>
                {:else if why.length}{why.slice(0, 2).join(" · ") + (why.length > 2 ? ` · +${why.length - 2} more` : "")}
                {:else if it.kind === "pr" && it.prs[0].issues.length}{it.prs[0].isDraft ? "draft" : it.prs[0].state.toLowerCase()} · fixes {it.prs[0].issues.map((n) => issueLabel(repo, n)).join(", ")}
                {:else if it.kind === "pr"}{it.prs[0].isDraft ? "draft" : it.prs[0].state.toLowerCase()} · standalone PR, fixes no Issue
                {:else if it.kind === "issue" && it.total === 0}no PR yet
                {:else}{it.open} open · {it.mergedCount} merged{/if}
              </span>
            </button>
            <span class="acts">
              {#if it.hidden === "author"}<button onclick={() => unmuteAuthor(it.author!)} title="Unmute {it.author} in {repo.name}">Unmute author</button>
              {:else if it.hidden}<button onclick={() => unhide(it.key)}>{it.hidden === "archived" ? "Unarchive" : "Unmute"}</button>
              {:else}
                <button class="ic" onclick={() => archive(it.key, label(it))} title="Archive (E): until something new needs you">⤓</button>
                <button class="ic" onclick={() => mute(it.key, label(it))} title="Mute (M): until you unmute it">🔇</button>
              {/if}
              {#if it.kind === "issue"}<button class="pin" class:on={it.pinned} onclick={() => togglePin(it.number)} title="Pin or unpin (Ctrl+K P)">{it.pinned ? "★" : "☆"}</button>{/if}
            </span>
          </div>
        </div>
      {:else}<div class="empty">{scope.kind === "archived" ? "Nothing archived. E archives the selected item." : scope.kind === "muted" ? (my.mutedAuthors.size ? "They hide nothing here right now." : "Nothing muted. M mutes the selected item.") : "Nothing here."}</div>{/each}
      {#if board.quiet.length}
        <button class="quietbtn" onclick={() => (showQuiet = !showQuiet)}>{showQuiet ? "Hide" : "Show"} {board.quiet.length} quiet items</button>
      {/if}
      {#if scope.kind === "issues" && !repo.small}
        <button class="quietbtn" onclick={() => say("Would fetch the next page of older Issues and PRs from GitHub (prototype: the snapshot has the latest page only)")}>Load older…</button>
      {/if}
    </div>
    <!-- The keys for the list, where the eye is when they are needed. -->
    {#if value("appearance.list_key_hints")}<footer class="legend">
      <span><i class="bar"></i> needs attention</span><span>★ pinned</span><span class="muted">↑ ↓ move · E archive · M mute · Alt+← back · ? all keys</span>
    </footer>{/if}
  </section>

  <!-- Clicks are on the links inside; this only routes them. -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <section class="right" onclick={onLink}>
    {#if selKind === "i"}
      <div class="scroll"><IssueView n={selNum} onPr={(n) => select(`p:${n}`)} onAuthor={showAuthor} /></div>
    {:else if sel === "deps" && depsItem}
      <div class="scroll"><DepsView item={depsItem} onPr={(n) => select(`p:${n}`)} /></div>
    {:else if current}
      {@const stack = wb.stack(current.number)}
      <header class="prhead">
        <div class="crumb">
          {#each current.issues as n, k}{#if k} · {/if}<button class="link" onclick={() => select(`i:${n}`)}>{issueLabel(repo, n)}</button>{/each}{#if !current.issues.length}Standalone PR{/if}
          › #{current.number} · <button class="link who" onclick={() => showAuthor(current.author)} title="Show {current.author}'s Issues and PRs">{current.author}</button> → <span class="mono">{current.baseRefName}</span>{current.kind === "bot" ? ` · bot backport of #${current.origin}` : current.kind === "manual" ? ` · manual backport of #${current.origin}` : ""} · updated {ago(current.updatedAt)} ago
        </div>
        <h2><a class="ghtitle" href={current.url} data-web title="Open on GitHub (Ctrl+Shift+G)">{current.short}<span class="ext">↗</span></a></h2>
        <div class="row">
          <StateDot pr={current} /><span>{current.isDraft ? "draft" : current.state.toLowerCase()}</span>
          <PrStatus pr={current} />
          <span class="muted">+{current.additions} −{current.deletions} · {current.changedFiles} files · {current.commitCount} commits</span>
        </div>
        {#if stack.length}
          <div class="stack">Stack:
            {#each stack as s, k}{#if k}<span class="muted">›</span>{/if}<button class="link" class:cur={s.number === current.number} onclick={() => select(`p:${s.number}`)} title={s.short}><StateDot pr={s} />#{s.number}</button>{/each}
            {#if current.stackIssue === "orphaned"}<span class="warn">⚠ #{current.stackedOn} has merged; this PR still targets its branch <span class="mono">{current.baseRefName}</span> and won't reach {repo.defaultBranch}. Retarget it on GitHub.</span>
            {:else if current.stackIssue === "merged-orphaned"}<span class="warn">⚠ Merged into <span class="mono">{current.baseRefName}</span> after #{current.stackedOn} had merged: its commits never reached {repo.defaultBranch}.</span>{/if}
          </div>
        {/if}
        <div class="row"><ReviewBar pr={current} /></div>
        <div class="tabs">
          <button class:on={tab === "conv"} onclick={() => (tab = "conv")}>Conversation</button>
          <button class:on={tab === "files"} onclick={() => (tab = "files")}>Files changed {current.changedFiles}</button>
          <span class="muted keys">Enter switches tab · F7 / F8 jump</span>
        </div>
      </header>
      <div class="scroll">
        {#if tab === "conv"}<Conversation pr={current} />{:else}<DiffView bind:this={diff} pr={current} {split} />{/if}
      </div>
    {:else}
      <div class="placeholder">Pick an item (↑ ↓). Ctrl+P opens anything, Ctrl+R switches Repository, ? lists keys.</div>
    {/if}
  </section>
</div>

<Palette bind:text={palette} {commands} files={current?.detail?.files.map((f) => f.path) ?? []} onPr={(n) => select(`p:${n}`)} onIssue={(n) => select(`i:${n}`)} onFile={() => (tab = "files")} />
<Peek bind:target={peek} onOpenInApp={openElsewhere} />
<Help bind:open={help} extra={[["↑ ↓", "Move through the Issue list"], ["Enter", "Switch Conversation / Files"]]} />

<style>
  .a { display: grid; grid-template-columns: 230px 430px 1fr; height: 100%; }
  .a.noside { grid-template-columns: 430px 1fr; }
  .left { border-right: 1px solid var(--line); background: var(--panel); padding: 8px; overflow: auto; display: flex; flex-direction: column; gap: 1px; }
  .repohead { display: flex; gap: 4px; align-items: center; padding: 2px 0 2px; }
  .repo { all: unset; flex: 1; min-width: 0; display: flex; gap: 4px; padding: 4px 8px; border-radius: 4px; cursor: pointer; font-weight: 700; font-size: 13px; }
  .repo:hover { background: var(--hover); }
  .rn { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .home { padding: 0 6px; }
  .role { font-size: 11px; color: var(--muted); padding: 0 8px 4px; }
  .sec { font-size: 11px; text-transform: uppercase; letter-spacing: 0.04em; color: var(--muted); padding: 12px 8px 4px; }
  .nav { all: unset; display: flex; justify-content: space-between; padding: 4px 8px; border-radius: 4px; cursor: pointer; font-size: 13px; }
  .nav:hover { background: var(--hover); } .nav.on { background: var(--sel); font-weight: 600; }
  .n { color: var(--muted); font-weight: 400; }
  .mid { border-right: 1px solid var(--line); display: flex; flex-direction: column; min-height: 0; }
  .list { flex: 1; overflow: auto; padding-bottom: 60px; }
  .lhead { display: flex; align-items: center; gap: 8px; padding: 8px 10px; border-bottom: 1px solid var(--line); font-size: 14px; }
  .search { display: flex; gap: 6px; padding: 6px 10px; border-bottom: 1px solid var(--line); }
  .search input { flex: 1; font: 13px var(--sans); padding: 4px 8px; background: var(--bg); color: var(--fg); border: 1px solid var(--line); border-radius: 4px; }
  .search input:focus { outline: none; border-color: var(--accent); }
  .quick { display: flex; gap: 6px; padding: 6px 10px; border-bottom: 1px solid var(--line); }
  .chip { display: inline-flex; align-items: center; gap: 5px; font: 12px var(--sans); padding: 2px 10px; border-radius: 12px; border: 1px solid var(--line); background: none; color: var(--muted); cursor: pointer; }
  .chip.on { color: var(--fg); border-color: var(--accent); background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .chip:not(.on) :global(.k) { color: var(--muted); }
  .chip .n { font-size: 11px; color: var(--muted); }
  .group { font-size: 11px; text-transform: uppercase; letter-spacing: 0.04em; color: var(--muted); padding: 10px 12px 4px; background: var(--panel); border-bottom: 1px solid var(--line); }
  .mutedrow { display: flex; justify-content: space-between; align-items: center; padding: 6px 12px; border-bottom: 1px solid var(--line); font-size: 13px; }
  .empty { padding: 16px 12px; color: var(--muted); font-size: 13px; }
  .grow { flex: 1; }
  .sort { all: unset; font-size: 12px; color: var(--muted); cursor: pointer; }
  .tmp { cursor: default; align-items: center; }
  .tmpact { display: flex; gap: 4px; }
  .tmpact button { padding: 0 6px; font-size: 11px; }
  .who { font-family: var(--sans); color: var(--fg); }
  .hint { font-size: 11px; color: var(--muted); padding: 14px 8px; }
  .issue { border-bottom: 1px solid var(--line); border-left: 3px solid transparent; }
  .issue.att { border-left-color: var(--accent); }
  .item { display: flex; align-items: flex-start; position: relative; }
  .item.sel { background: var(--sel); }
  .item:hover { background: var(--hover); } .item.sel:hover { background: var(--sel); }
  .ibody { all: unset; flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; padding: 7px 10px 7px 12px; cursor: pointer; }
  .l1 { font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; padding-right: 70px; }
  .l2 { display: flex; gap: 6px; align-items: baseline; font-size: 13px; }
  .ititle { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .att .ititle { color: var(--accent); font-weight: 600; }
  .att .l1 { font-weight: 600; }
  .cnt { color: var(--muted); font-size: 12px; flex: none; }
  .time { color: var(--muted); font-size: 12px; flex: none; }
  .att .time { color: var(--accent); }
  .l3 { font-size: 12px; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .inum { font: 12px var(--mono); }
  .ititle :global(.k) { margin-right: 4px; }
  .deps .inum { font-family: var(--sans); font-style: italic; }
  .acts { position: absolute; right: 8px; top: 5px; display: flex; gap: 2px; align-items: center; }
  .acts .ic { all: unset; cursor: pointer; color: var(--muted); font-size: 12px; padding: 0 3px; visibility: hidden; }
  .item:hover .ic, .item.sel .ic { visibility: visible; }
  .acts .ic:hover { color: var(--accent); }
  .acts button:not(.ic):not(.pin) { font-size: 11px; padding: 0 6px; }
  .pin { all: unset; cursor: pointer; color: var(--muted); padding: 0 2px; } .pin.on { color: var(--warn); }
  .bar { display: inline-block; width: 3px; height: 10px; background: var(--accent); vertical-align: middle; }
  .legend { display: flex; gap: 10px; align-items: center; padding: 5px 10px; border-top: 1px solid var(--line); font-size: 11px; color: var(--fg); background: var(--panel); flex-wrap: wrap; }
  .legend span { display: inline-flex; gap: 4px; align-items: center; }
  .quietbtn { all: unset; display: block; padding: 10px; font-size: 12px; color: var(--muted); cursor: pointer; }
  .right { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .prhead { padding: 10px 16px 0; border-bottom: 1px solid var(--line); }
  .crumb { font-size: 12px; color: var(--muted); }
  .link { all: unset; color: var(--accent); cursor: pointer; font-family: var(--mono); display: inline-flex; gap: 4px; align-items: center; }
  .link:hover { text-decoration: underline; }
  .link.cur { font-weight: 700; color: var(--fg); }
  h2 { margin: 4px 0 6px; font-size: 18px; }
  .row { display: flex; gap: 8px; align-items: center; margin-bottom: 6px; font-size: 12px; }
  .stack { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; font-size: 12px; margin-bottom: 6px; }
  .warn { color: var(--bad); }
  .tabs { display: flex; gap: 2px; align-items: end; }
  .tabs button { all: unset; padding: 6px 12px; cursor: pointer; font-size: 13px; border-bottom: 2px solid transparent; }
  .tabs button.on { border-color: var(--accent); font-weight: 600; }
  .keys { margin-left: auto; font-size: 11px; padding-bottom: 6px; }
  .scroll { flex: 1; overflow: auto; padding: 12px 16px 60px; }
  .placeholder { margin: auto; color: var(--muted); }
  .muted { color: var(--muted); font-weight: 400; }
  .mono { font-family: var(--mono); }
</style>
