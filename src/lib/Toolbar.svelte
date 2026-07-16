<script lang="ts">
  let {
    sidebarCollapsed = false,
    sidebarWidth = 230,
    onToggleSidebar,
    onRefresh,
    onCompose,
    onSearch,
    onOpenSettings,
  }: {
    sidebarCollapsed?: boolean;
    /** Current sidebar pane width — the traffic-light zone tracks it. */
    sidebarWidth?: number;
    onToggleSidebar: () => void;
    onRefresh: () => void;
    onCompose: () => void;
    onSearch: (query: string) => void;
    onOpenSettings: () => void;
  } = $props();

  let query = $state("");

  // Collapsed, the zone keeps just enough room for the native traffic
  // lights plus the toggle button.
  let zoneWidth = $derived(sidebarCollapsed ? 130 : sidebarWidth);
</script>

<!-- why: with titleBarStyle Overlay there is no native title bar left to grab,
     so the toolbar itself is the window drag handle. -->
<div class="toolbar" data-tauri-drag-region>
  <!-- Native traffic lights float over this zone's left end
       (tauri.conf.json trafficLightPosition); the toggle sits at its
       right edge, next to the sidebar it controls. -->
  <div
    class="sidebar-zone"
    style:width={`${zoneWidth}px`}
    data-tauri-drag-region
  >
    <button
      class="icon-btn"
      aria-label="Toggle sidebar"
      title="Toggle sidebar"
      onclick={onToggleSidebar}
    >
      <svg viewBox="0 0 20 20" aria-hidden="true">
        <rect x="2.5" y="3.5" width="15" height="13" rx="2.5" />
        <line x1="7.5" y1="3.5" x2="7.5" y2="16.5" />
      </svg>
    </button>
  </div>

  <div class="actions" data-tauri-drag-region>
    <button
      class="action"
      aria-label="Check for new mail"
      title="Check for new mail"
      onclick={onRefresh}
    >
      <svg viewBox="0 0 20 20" aria-hidden="true">
        <path d="M16.5 10a6.5 6.5 0 1 1-1.9-4.6" />
        <path d="M16.8 2.8v3.4h-3.4" />
      </svg>
      <span class="label">Refresh</span>
    </button>
    <button
      class="action"
      aria-label="New Message"
      title="New Message"
      onclick={onCompose}
    >
      <svg viewBox="0 0 20 20" aria-hidden="true">
        <path d="M10 3.5H5a2 2 0 0 0-2 2V15a2 2 0 0 0 2 2h9.5a2 2 0 0 0 2-2v-5" />
        <path d="M15.8 2.7a1.6 1.6 0 0 1 2.3 2.3L11 12.1l-3 .7.7-3z" />
      </svg>
      <span class="label">New</span>
    </button>
  </div>

  <div class="search-zone">
    <div class="field">
      <svg class="glass" viewBox="0 0 20 20" aria-hidden="true">
        <circle cx="9" cy="9" r="6" />
        <line x1="13.5" y1="13.5" x2="17.5" y2="17.5" />
      </svg>
      <input
        type="search"
        placeholder="Search"
        aria-label="Search messages"
        title="Narrow with from:… to:… subject:… is:unread"
        bind:value={query}
        oninput={() => onSearch(query)}
      />
    </div>
    <button
      class="icon-btn"
      aria-label="Settings"
      title="Settings"
      onclick={onOpenSettings}
    >
      <svg viewBox="0 0 20 20" aria-hidden="true">
        <circle cx="10" cy="10" r="2.6" />
        <path
          d="M10 2.8v2.1M10 15.1v2.1M2.8 10h2.1M15.1 10h2.1M4.9 4.9l1.5 1.5M13.6 13.6l1.5 1.5M15.1 4.9l-1.5 1.5M6.4 13.6l-1.5 1.5"
        />
      </svg>
    </button>
  </div>
</div>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    box-sizing: border-box;
    height: var(--toolbar-height);
    background: linear-gradient(
      var(--bg-toolbar-top),
      var(--bg-toolbar-bottom)
    );
    border-bottom: 1px solid var(--border-chrome);
  }

  .sidebar-zone {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex-shrink: 0;
    box-sizing: border-box;
    padding: 0 12px;
    transition: width 0.2s ease;
  }

  svg {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
  }

  .icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 34px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text-secondary);
    cursor: default;
  }

  .icon-btn:hover {
    background: var(--bg-hover);
  }

  .icon-btn svg {
    width: 18px;
    height: 18px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 2px;
    flex: 1;
    min-width: 0;
    padding: 0 10px;
    overflow: hidden;
  }

  .action {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 1px;
    flex-shrink: 0;
    min-width: 38px;
    height: 44px;
    padding: 0 8px;
    border: none;
    border-radius: 6px;
    background: transparent;
    font: inherit;
    color: var(--text-secondary);
    white-space: nowrap;
    cursor: default;
  }

  .action:hover {
    background: var(--bg-hover);
  }

  .action svg {
    width: 18px;
    height: 18px;
  }

  .action .label {
    font-size: 10px;
  }

  .search-zone {
    display: flex;
    align-items: center;
    gap: 4px;
    flex: 0 1 322px;
    min-width: 170px;
    box-sizing: border-box;
    padding: 0 14px;
  }

  .field {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    min-width: 0;
    padding: 5px 9px;
    border-radius: 7px;
    background: var(--bg-field);
  }

  .field .glass {
    flex-shrink: 0;
    width: 14px;
    height: 14px;
    stroke-width: 1.8;
    color: var(--text-tertiary);
  }

  .field input {
    width: 100%;
    padding: 0;
    border: none;
    background: transparent;
    outline: none;
    font: inherit;
    color: var(--text-primary);
  }
</style>
