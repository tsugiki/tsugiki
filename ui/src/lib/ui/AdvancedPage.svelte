<!-- Settings › Advanced (docs/devel/settings.md): for now its Language servers group (#683). Each server's
     row has the path of its program with Browse…, the versions we tested and the one its check found, its
     state or last problem, and Open log. The idle time comes after them. All of it is kept in
     `machine.toml`, and takes effect on Apply, which checks a new path and keeps the dialog open to show what
     the check found. -->
<script lang="ts">
  import { tick } from "svelte";
  import {
    LanguageServerRows,
    openServerLog,
    pickProgram,
    typedPath,
  } from "$lib/state/language-servers.svelte";
  import {
    SERVER_ORDER,
    type ServerName,
    SERVERS,
    statusWords,
    stderrTail,
    versionWords,
  } from "$lib/state/server-words";
  import type { SettingsDialog } from "$lib/state/settings-dialog.svelte";
  import StoredIn from "./StoredIn.svelte";

  let {
    dialog,
    repository,
    at,
  }: {
    dialog: SettingsDialog;
    /** The open Repository, whose servers' logs Open log opens, if one is open. */
    repository: string | undefined;
    /** The server whose row the dialog opened at, which takes the focus once the files are read. */
    at?: ServerName | undefined;
  } = $props();

  const rows = new LanguageServerRows();
  $effect(() => rows.watch());

  /** What went wrong with Browse… or Open log on each server's row, until the next try. */
  let problems = $state<Partial<Record<ServerName, string | undefined>>>({});
  /** Whether the row asked for has had the focus, which it takes once. */
  let reached = false;

  /** Gives the path input of the row asked for the focus, once the dialog has read the files and turned it
   * on. */
  const reach = (server: ServerName) => (input: HTMLInputElement) => {
    if (reached || server !== at || dialog.servers === undefined) return;
    reached = true;
    void tick().then(() => {
      input.focus();
    });
  };

  /** Sets `server`'s path as `typedPath` reads `path`, or clears it when `path` is blank, and gives what it
   * set. */
  function setPath(server: ServerName, path: string): string | null {
    const set = typedPath(path);
    dialog.changeServers((servers) => {
      servers[server] = set;
    });
    return set;
  }

  async function browse(server: ServerName) {
    problems[server] = undefined;
    const picked = await pickProgram();
    if ("problem" in picked) problems[server] = picked.problem;
    else if (picked.path !== null) setPath(server, picked.path);
  }

  async function openLog(server: ServerName, repository: string) {
    problems[server] = undefined;
    problems[server] = await openServerLog(repository, server);
  }

  /** Takes the idle time typed, if it is a whole number of minutes above 0, and otherwise shows the one in
   * force again. */
  function setIdle(input: HTMLInputElement) {
    const minutes = Number(input.value);
    dialog.changeServers((servers) => {
      if (Number.isInteger(minutes) && minutes >= 1) servers.idle_minutes = minutes;
      else input.value = String(servers.idle_minutes);
    });
  }
</script>

<h3>Language servers</h3>
{#if rows.problem !== undefined}
  <p role="alert">{rows.problem}</p>
{/if}
{#each SERVER_ORDER as server (server)}
  {@const { name, language } = SERVERS[server]}
  {@const row = rows.row(server)}
  <div class={["row", { asked: server === at }]} role="group" aria-label={name}>
    <div class="title">
      <!-- A template string: Svelte compiles ` for {language}` with a `?? ""` branch no test can reach. -->
      <label for={`server-${server}`}><strong>{name}</strong>{` for ${language}`}</label>
      <StoredIn where="machine" />
    </div>
    <div class="path">
      <input
        id={`server-${server}`}
        class="mono"
        type="text"
        spellcheck="false"
        placeholder="The path of its program"
        value={dialog.servers?.[server] ?? ""}
        disabled={dialog.servers === undefined}
        onchange={(e) => {
          // The input shows the path as it was set, though that is the path it showed before.
          e.currentTarget.value = setPath(server, e.currentTarget.value) ?? "";
        }}
        {@attach reach(server)}
      />
      <button disabled={dialog.servers === undefined} onclick={() => void browse(server)}>Browse…</button>
    </div>
    {#if row !== undefined}
      {@const version = versionWords(row)}
      {@const tail = stderrTail(row.status)}
      <p class={["help", { warning: version.untested }]}>{version.words}</p>
      <p class={["state", { trouble: row.status.state === "problem" }]}>{statusWords(server, row.status)}</p>
      {#if tail !== undefined}
        <pre class="tail" aria-label={`What ${name} wrote last`}>{tail}</pre>
      {/if}
    {/if}
    {#if repository === undefined}
      <button disabled title="Open a Repository to open the log its server writes">Open log</button>
    {:else}
      {@const open = repository}
      <button title={`The log ${name} writes for ${open}`} onclick={() => void openLog(server, open)}
        >Open log</button
      >
    {/if}
    {#if problems[server] !== undefined}
      <p role="alert">{problems[server]}</p>
    {/if}
  </div>
{/each}
<div class="row">
  <div class="title">
    <label for="idle-minutes">Stop a server no lookup has used for</label>
    <input
      id="idle-minutes"
      class="minutes"
      type="number"
      min="1"
      step="1"
      value={dialog.servers?.idle_minutes}
      disabled={dialog.servers === undefined}
      onchange={(e) => {
        setIdle(e.currentTarget);
      }}
    />
    minutes
    <StoredIn where="machine" />
  </div>
  <p class="help">The next lookup starts it again.</p>
</div>

<style>
  h3 {
    margin: 4px 0 8px;
    padding-bottom: 4px;
    font-size: 13px;
    border-bottom: 1px solid var(--border);
  }

  .row {
    margin-bottom: 14px;
    padding: 4px 6px;
    border-left: 2px solid transparent;
  }

  .row.asked {
    border-left-color: var(--accent);
  }

  .title {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .path {
    display: flex;
    gap: 6px;
    margin: 4px 0;
  }

  .path input {
    flex: 1;
    min-width: 0;
  }

  .minutes {
    width: 5em;
  }

  p {
    margin: 3px 0;
  }

  .help {
    font-size: 12px;
    color: var(--fg-muted);
  }

  /* Tsugiki still uses a version we didn't test. */
  .warning {
    color: var(--attention);
  }

  .trouble,
  [role="alert"] {
    color: var(--danger);
  }

  .tail {
    max-height: 8em;
    margin: 3px 0;
    padding: 4px 6px;
    overflow: auto;
    font-size: 11.5px;
    white-space: pre-wrap;
    background: color-mix(in srgb, currentColor 6%, transparent);
    border-radius: 4px;
  }
</style>
