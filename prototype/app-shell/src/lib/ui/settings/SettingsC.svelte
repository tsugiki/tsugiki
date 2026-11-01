<script lang="ts">
  // PROTOTYPE — throwaway. Variant C, "The files": no form at all. The two files are edited as text,
  // the way they are synced and diffed. On the right: what the line under the cursor means, the
  // problems (valid lines still apply), and every setting there is. Only the token is a field, since
  // it never goes in a file.
  import { ACTIONS, CONVENTIONS, FIRST_SYNC, SETTINGS, SECTIONS, settingDef } from "$lib/core/settings";
  import { configText, localText, applyFile, secret, value } from "$lib/state/settings.svelte";
  import { prettyKey } from "../keys";
  import Where from "./Where.svelte";
  let { onClose }: { onClose: () => void } = $props();

  let tab = $state<"config" | "local">("config");
  const texts = $state({ config: configText(), local: localText() });
  const problems = $state<{ config: string[]; local: string[] }>({ config: [], local: [] });
  let area = $state<HTMLTextAreaElement>();
  let gutter = $state<HTMLElement>();
  let caretLine = $state(1);
  let timer: ReturnType<typeof setTimeout> | undefined;

  function edit(text: string) {
    texts[tab] = text;
    const which = tab;
    clearTimeout(timer);
    timer = setTimeout(() => { problems[which] = applyFile(which, texts[which]); }, 250);
  }
  function caret() { if (area) caretLine = area.value.slice(0, area.selectionStart).split("\n").length; }
  function goLine(n: number) {
    if (!area) return;
    const lines = area.value.split("\n");
    const at = lines.slice(0, n - 1).reduce((a, l) => a + l.length + 1, 0);
    area.focus();
    area.setSelectionRange(at, at + lines[n - 1].length);
    area.scrollTop = Math.max(0, (n - 5) * 18);
    caretLine = n;
  }
  const lineCount = $derived(texts[tab].split("\n").length);
  // Open each file at its top, cursor on line 1.
  $effect(() => { tab; if (area) { area.setSelectionRange(0, 0); area.scrollTop = 0; caretLine = 1; } });

  /** What the line under the cursor is: a setting, a key, a Repository field, or a table. */
  const REPO_DOC: Record<string, string> = {
    name: "The Repository, owner/name.",
    conventions: `How PRs find their Issues: ${CONVENTIONS.map(([v, l]) => `"${v}" (${l})`).join(", ")}.`,
    first_sync: `${FIRST_SYNC.map(([v, l]) => `"${v}" (${l})`).join(", ")}.`,
    pinned: "Pinned issues, by number. Ctrl+K P in the app adds one.",
    muted_authors: "Logins whose Issues and PRs are hidden in this Repository. A Review request or an assignment still shows.",
    q: "The Filter's GitHub search.",
    clone: "A local clone to borrow objects from, read-only. Saves the first download; never written to.",
  };
  const here = $derived.by(() => {
    const lines = texts[tab].split("\n");
    let table = "";
    for (let i = 0; i < caretLine; i++) { const m = /^\s*\[\[?([^\]]+)\]\]?/.exec(lines[i]); if (m) table = m[1].trim(); }
    const key = /^\s*([\w.-]+)\s*=/.exec(lines[caretLine - 1] ?? "")?.[1];
    if (!key) return table ? { title: `[${table}]`, body: table.startsWith("repository") ? "A Repository's settings." : table === "keys" ? "Remapped keys: action = \"key\". The rest stay the defaults." : "" } : null;
    const def = settingDef(`${table}.${key}`);
    if (def) return { title: def.label, def };
    if (table === "keys") { const a = ACTIONS.find((x) => x.id === key); return a ? { title: a.label, body: `Default: ${prettyKey(a.key)}. A chord is two keys with a space: "ctrl+k m".` } : { title: key, body: "Not an action gh-rev knows." }; }
    if (table.startsWith("repository")) return { title: key, body: REPO_DOC[key] ?? "Not a Repository setting." };
    return { title: key, body: "Not a setting gh-rev knows." };
  });
  /** The line a problem is about: given ("line 19: …"), or the line that sets `table.key`. */
  function lineOf(p: string): number {
    const given = /^line (\d+)/.exec(p);
    if (given) return +given[1];
    const m = /^([\w]+)\.([\w-]+):/.exec(p) ?? /^\[([\w]+)\]:/.exec(p);
    if (!m) return 0;
    const lines = texts[tab].split("\n");
    let inTable = false;
    for (let i = 0; i < lines.length; i++) {
      const t = /^\s*\[([^\]]+)\]/.exec(lines[i]);
      if (t) { inTable = t[1].trim() === m[1]; if (inTable && !m[2]) return i + 1; continue; }
      if (inTable && m[2] && new RegExp(`^\\s*${m[2]}\\s*=`).test(lines[i])) return i + 1;
    }
    return 0;
  }
</script>

<div class="c">
  <header>
    <b>Settings</b><span class="muted">are these two files. Edit them here or in any editor; sync config.toml yourself.</span>
    <span class="grow"></span>
    <button onclick={onClose} title="Close (Esc)">✕</button>
  </header>
  <div class="ed">
    <div class="tabs">
      <button class:on={tab === "config"} onclick={() => { tab = "config"; caretLine = 1; }}>config.toml <Where where="synced" />{#if problems.config.length}<span class="nerr">{problems.config.length}</span>{/if}</button>
      <button class:on={tab === "local"} onclick={() => { tab = "local"; caretLine = 1; }}>local.toml <Where where="machine" />{#if problems.local.length}<span class="nerr">{problems.local.length}</span>{/if}</button>
      <span class="grow"></span>
      <span class="path mono">{tab === "config" ? value("storage.config_file") : String(value("storage.config_file")).replace(/config\.toml$/, "local.toml")}</span>
    </div>
    <div class="code">
      <div class="gutter" bind:this={gutter}>{#each { length: lineCount } as _, i}<div class:cur={i + 1 === caretLine} class:bad={problems[tab].some((p) => lineOf(p) === i + 1)}>{i + 1}</div>{/each}</div>
      <!-- svelte-ignore a11y_autofocus -->
      <textarea bind:this={area} value={texts[tab]} spellcheck="false" autofocus
        oninput={(e) => { edit(e.currentTarget.value); caret(); }} onkeyup={caret} onclick={caret}
        onscroll={(e) => { if (gutter) gutter.scrollTop = e.currentTarget.scrollTop; }}></textarea>
    </div>
    <div class="token">
      <span>Personal access token <Where where="keychain" /></span>
      <input type="password" bind:value={secret.token} placeholder={value("auth.source") === "token" ? "ghp_… or github_pat_…" : "Not used: auth.source = \"gh\" in local.toml"} disabled={value("auth.source") !== "token"} autocomplete="off" />
    </div>
  </div>
  <aside>
    <div class="box">
      <div class="h">At the cursor</div>
      {#if here?.def}
        <b>{here.def.label}</b> <Where where={here.def.where} />
        <p>{here.def.help}</p>
        <p class="muted">
          {#if here.def.type.kind === "enum"}One of {here.def.type.options.map(([o, l]) => `"${o}" (${l})`).join(", ")}.
          {:else if here.def.type.kind === "number"}A number, {here.def.type.min}–{here.def.type.max}{here.def.type.unit ? ` ${here.def.type.unit}` : ""}.
          {:else if here.def.type.kind === "bool"}true or false.{/if}
          Default: <code>{JSON.stringify(here.def.default)}</code>.
        </p>
      {:else if here}<b>{here.title}</b><p>{here.body}</p>
      {:else}<p class="muted">Put the cursor on a line to see what it does.</p>{/if}
    </div>
    <div class="box">
      <div class="h">Problems in {tab === "config" ? "config.toml" : "local.toml"}</div>
      {#each problems[tab] as p}<button class="prob" onclick={() => lineOf(p) && goLine(lineOf(p))}>{p}</button>
      {:else}<p class="ok">✓ None. Every valid line is already in effect.</p>{/each}
    </div>
    <div class="box ref">
      <div class="h">Everything you can set</div>
      {#each SECTIONS as s}
        {#each SETTINGS.filter((x) => x.section === s && x.where !== "keychain") as x}
          <div class="refrow"><code>{x.key}</code> <Where where={x.where} /><span class="muted">{x.label}</span></div>
        {/each}
      {/each}
      <div class="refrow"><code>keys.&lt;action&gt;</code> <Where where="synced" /><span class="muted">{ACTIONS.length} actions: {ACTIONS.slice(0, 4).map((a) => a.id).join(", ")}, …</span></div>
      <div class="refrow"><code>[[repository]]</code> <Where where="synced" /><span class="muted">name, conventions, first_sync, pinned, muted_authors, [[repository.filter]] name / q</span></div>
      <div class="refrow"><code>[repository."owner/name"]</code> <Where where="machine" /><span class="muted">clone</span></div>
    </div>
  </aside>
</div>

<style>
  .c { position: fixed; inset: 0; z-index: 40; background: var(--bg); display: grid; grid-template-columns: 1fr 380px; grid-template-rows: auto 1fr; font-size: 13px; }
  header { grid-column: 1 / -1; display: flex; gap: 10px; align-items: center; padding: 8px 14px; border-bottom: 1px solid var(--line); background: var(--panel); }
  .grow { flex: 1; }
  .muted { color: var(--muted); }
  .mono { font-family: var(--mono); font-size: 11.5px; }
  .ed { display: flex; flex-direction: column; min-height: 0; }
  .tabs { display: flex; align-items: center; gap: 2px; border-bottom: 1px solid var(--line); padding-right: 10px; }
  .tabs button { all: unset; cursor: pointer; padding: 6px 12px; display: inline-flex; gap: 6px; align-items: center; border-bottom: 2px solid transparent; }
  .tabs button.on { border-bottom-color: var(--accent); font-weight: 600; }
  .path { color: var(--muted); }
  .nerr { background: var(--bad); color: #fff; border-radius: 999px; font-size: 10.5px; padding: 0 6px; }
  .code { flex: 1; display: flex; min-height: 0; font: 13px/18px var(--mono); }
  .gutter { width: 44px; overflow: hidden; text-align: right; padding: 8px 8px 8px 0; color: var(--muted); background: var(--panel); user-select: none; box-sizing: border-box; }
  .gutter div { height: 18px; }
  .gutter .cur { color: var(--fg); }
  .gutter .bad { color: var(--bad); font-weight: 700; }
  textarea { flex: 1; resize: none; border: 0; outline: none; padding: 8px 10px; font: inherit; color: var(--fg); background: var(--bg); white-space: pre; overflow: auto; tab-size: 4; }
  .token { display: flex; gap: 10px; align-items: center; padding: 8px 12px; border-top: 1px solid var(--line); background: var(--panel); }
  .token span { display: inline-flex; gap: 6px; align-items: center; }
  .token input { flex: 1; max-width: 360px; font: 13px var(--sans); padding: 3px 6px; border: 1px solid var(--line); border-radius: 4px; background: var(--bg); color: var(--fg); }
  aside { border-left: 1px solid var(--line); overflow: auto; padding: 10px 14px; display: flex; flex-direction: column; gap: 14px; }
  .h { font-size: 11px; text-transform: uppercase; letter-spacing: 0.04em; color: var(--muted); margin-bottom: 6px; }
  .box p { margin: 6px 0; }
  .prob { all: unset; display: block; cursor: pointer; color: var(--bad); font-size: 12.5px; padding: 2px 0; }
  .prob:hover { text-decoration: underline; }
  .ok { color: var(--good); }
  .refrow { display: flex; gap: 6px; align-items: baseline; flex-wrap: wrap; padding: 2px 0; font-size: 12px; }
  code { font: 11.5px var(--mono); }
</style>
