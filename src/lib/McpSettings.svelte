<script lang="ts">
  import { onMount } from "svelte";
  import {
    copyMcpSetup,
    getMcpSettings,
    mcpStatus,
    regenerateMcpToken,
    setMcpSettings,
  } from "./api";
  import type { McpAccess, McpSettings, McpSetupKind, McpStatus } from "./types";

  const ACCESS: { value: McpAccess; label: string; explain: string }[] = [
    {
      value: "read",
      label: "Read mail",
      explain:
        "Accounts, folders, messages, threads, search and attachments. Reading does not mark mail read.",
    },
    {
      value: "organize",
      label: "Read and organize",
      explain:
        "Also mark read or unread, archive, move to Trash or another folder, and save drafts.",
    },
    {
      value: "full",
      label: "Full — also send",
      explain:
        "Also send, reply and forward, without asking you first. Each message waits a few seconds in the outbox, where Undo stops it.",
    },
  ];

  let settings = $state<McpSettings | null>(null);
  let status = $state<McpStatus | null>(null);
  let error = $state<string | null>(null);
  let copied = $state<McpSetupKind | null>(null);

  let access = $derived(ACCESS.find((a) => a.value === settings?.access));

  /** Save, restart the server, show its new status — or roll back. */
  async function save(next: McpSettings) {
    if (!settings) return;
    error = null;
    const previous = settings;
    settings = next;
    try {
      status = await setMcpSettings(next);
    } catch (err) {
      // why: the controls must not lie — a failed save puts them back.
      settings = previous;
      error = String(err);
    }
  }

  function savePort(raw: string) {
    if (!settings) return;
    const port = Number(raw);
    if (!Number.isInteger(port) || port < 1024 || port > 65535) {
      error = "Pick a port from 1024 to 65535.";
      return;
    }
    void save({ ...settings, port });
  }

  async function copy(kind: McpSetupKind) {
    error = null;
    try {
      await copyMcpSetup(kind);
      copied = kind;
    } catch (err) {
      error = String(err);
    }
  }

  async function newToken() {
    error = null;
    copied = null;
    try {
      status = await regenerateMcpToken();
    } catch (err) {
      error = String(err);
    }
  }

  onMount(() => {
    void getMcpSettings().then((stored) => (settings = stored));
    void mcpStatus().then((current) => (status = current));
  });
</script>

{#if settings}
  <fieldset>
    <legend>MCP server</legend>
    <div class="choice">
      <input
        type="checkbox"
        id="mcp-enabled"
        checked={settings.enabled}
        onchange={(e) =>
          settings && void save({ ...settings, enabled: e.currentTarget.checked })}
      />
      <span>
        <label for="mcp-enabled">Let AI apps use Flit through MCP</label>
        <small>
          Claude, Cursor and other MCP clients you connect can read your mail
          and, if you allow it, act on it.
        </small>
      </span>
    </div>
    <p class="explain">
      Experimental. What a connected app reads leaves Flit: it goes to that
      app, and usually to its maker's cloud model. Flit itself contacts
      nobody — it only answers apps on this Mac that hold the token. Mail is
      written by strangers, and a message can try to talk an AI into things.
    </p>

    {#if settings.enabled}
      <div class="setting">
        <label for="mcp-access">Access</label>
        <select
          id="mcp-access"
          value={settings.access}
          onchange={(e) =>
            settings &&
            void save({ ...settings, access: e.currentTarget.value as McpAccess })}
        >
          {#each ACCESS as option (option.value)}
            <option value={option.value}>{option.label}</option>
          {/each}
        </select>
      </div>
      {#if access}
        <p class="explain">{access.explain}</p>
      {/if}

      <div class="setting">
        <label for="mcp-port">Port</label>
        <input
          id="mcp-port"
          type="number"
          min="1024"
          max="65535"
          value={settings.port}
          onchange={(e) => savePort(e.currentTarget.value)}
        />
      </div>

      <p class="status">
        {#if status?.running && status.port}
          Listening on <code>http://127.0.0.1:{status.port}/mcp</code>
        {:else if status?.error}
          <span class="failed">{status.error}</span>
        {:else}
          Not running.
        {/if}
      </p>

      <div class="connect">
        <button class="action" onclick={() => void copy("claude-code")}>
          Copy Claude Code command
        </button>
        <button class="action" onclick={() => void copy("json")}>
          Copy JSON config
        </button>
        <button class="action" onclick={() => void copy("token")}>Copy token</button>
        <button class="action" onclick={() => void newToken()}>New token</button>
        {#if copied}
          <span class="copied">Copied</span>
        {/if}
      </div>
      <p class="explain">
        The command and the config carry the access token. Any app holding it
        can use the server while Flit runs; New token cuts every connected app
        off.
      </p>
    {/if}
  </fieldset>
{/if}

{#if error}
  <p class="error" role="alert">{error}</p>
{/if}

<style>
  /* The look of the other Experimental boxes (ExperimentalPane). */
  fieldset {
    margin: 0;
    max-width: 520px;
    padding: 8px 12px;
    border: 1px solid var(--hairline);
    border-radius: 9px;
    background: var(--bg-window);
  }

  legend {
    padding: 0 4px;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-secondary);
  }

  .choice {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 5px 0;
    font-size: 13px;
  }

  .choice input {
    margin-top: 2px;
  }

  .choice span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .choice label {
    font-weight: 500;
  }

  .choice small,
  .explain {
    font-size: 11.5px;
    color: var(--text-secondary);
  }

  .explain {
    margin: 6px 0 2px;
  }

  .setting {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 5px 0;
    font-size: 13px;
    font-weight: 500;
  }

  .setting label {
    width: 10rem;
  }

  select,
  input[type="number"] {
    padding: 3px 8px;
    border: 1px solid var(--border-chrome);
    border-radius: 6px;
    background: var(--bg-window);
    font: inherit;
    font-size: 12.5px;
    font-weight: 400;
  }

  select {
    min-width: 11rem;
  }

  input[type="number"] {
    width: 6rem;
    font-variant-numeric: tabular-nums;
  }

  .status {
    margin: 8px 0 4px;
    font-size: 12px;
  }

  code {
    font-size: 11.5px;
  }

  .failed {
    color: var(--danger);
  }

  .connect {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-top: 4px;
  }

  .action {
    padding: 2px 10px;
    border: 1px solid var(--border-chrome);
    border-radius: 6px;
    background: var(--bg-window);
    font: inherit;
    font-size: 11.5px;
    font-weight: 500;
    color: var(--text-primary);
    cursor: default;
  }

  .action:hover {
    background: var(--bg-hover);
  }

  .copied {
    font-size: 11.5px;
    color: var(--text-secondary);
  }

  .error {
    margin: 0;
    font-size: 12px;
    color: var(--danger);
  }
</style>
