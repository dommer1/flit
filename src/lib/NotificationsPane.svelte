<script lang="ts">
  import { onMount } from "svelte";
  import {
    getNotificationSettings,
    previewNotificationSound,
    setAccountNotifications,
    setNotificationSettings,
  } from "./api";
  import type { Account, NotificationSettings } from "./types";

  let { accounts }: { accounts: Account[] } = $props();

  /** The macOS system alert sounds (/System/Library/Sounds). */
  const SOUNDS = [
    "Basso",
    "Blow",
    "Bottle",
    "Frog",
    "Funk",
    "Glass",
    "Hero",
    "Morse",
    "Ping",
    "Pop",
    "Purr",
    "Sosumi",
    "Submarine",
    "Tink",
  ];

  const INTERVALS: { value: number; label: string }[] = [
    { value: 0, label: "Manually" },
    { value: 1, label: "Every minute" },
    { value: 3, label: "Every 3 minutes" },
    { value: 5, label: "Every 5 minutes" },
    { value: 15, label: "Every 15 minutes" },
    { value: 30, label: "Every 30 minutes" },
  ];

  // Mirrors the backend defaults until the stored values load.
  let settings = $state<NotificationSettings>({
    enabled: true,
    sound: "default",
    syncIntervalMinutes: 3,
  });
  let error = $state<string | null>(null);

  async function save(next: NotificationSettings) {
    error = null;
    const previous = settings;
    settings = next;
    try {
      await setNotificationSettings(next);
    } catch (err) {
      // why: the control must not lie — a failed save rolls the value back.
      settings = previous;
      error = String(err);
    }
  }

  async function saveAccount(
    account: Account,
    enabled: boolean | null,
    sound: string | null,
  ) {
    error = null;
    try {
      // The backend broadcasts accounts-changed; the parent refetches and
      // these rows re-render from the fresh account rows.
      await setAccountNotifications(account.id, enabled, sound);
    } catch (err) {
      error = String(err);
    }
  }

  onMount(() => {
    void getNotificationSettings().then((stored) => (settings = stored));
  });
</script>

<section class="pane">
  <fieldset>
    <legend>New mail</legend>
    <label class="setting">
      <input
        type="checkbox"
        checked={settings.enabled}
        onchange={(e) =>
          void save({ ...settings, enabled: e.currentTarget.checked })}
      />
      Show notifications
    </label>
    <div class="setting">
      <label for="notification-sound">Sound</label>
      <select
        id="notification-sound"
        value={settings.sound}
        onchange={(e) => {
          // Hear the pick right away; "default"/"none" play nothing.
          void previewNotificationSound(e.currentTarget.value);
          void save({ ...settings, sound: e.currentTarget.value });
        }}
      >
        <option value="default">System default</option>
        <option value="none">None</option>
        {#each SOUNDS as sound (sound)}
          <option value={sound}>{sound}</option>
        {/each}
      </select>
    </div>
    <div class="setting">
      <label for="notification-interval">Check for new mail</label>
      <select
        id="notification-interval"
        value={settings.syncIntervalMinutes}
        onchange={(e) =>
          void save({
            ...settings,
            syncIntervalMinutes: Number(e.currentTarget.value),
          })}
      >
        {#each INTERVALS as interval (interval.value)}
          <option value={interval.value}>{interval.label}</option>
        {/each}
      </select>
    </div>
    <p class="explain">
      New messages in each account's inbox show a notification. Checking also
      keeps the message list fresh while the app is open.
    </p>
  </fieldset>

  <fieldset>
    <legend>Per account</legend>
    <p class="explain">
      Accounts follow the settings above unless changed here.
    </p>
    {#each accounts as account (account.id)}
      <div class="account-row">
        <span class="account-name">{account.name}</span>
        <input
          type="checkbox"
          aria-label="Notifications for {account.name}"
          checked={account.notifyEnabled ?? settings.enabled}
          onchange={(e) =>
            void saveAccount(
              account,
              e.currentTarget.checked,
              account.notifySound,
            )}
        />
        <select
          aria-label="Sound for {account.name}"
          value={account.notifySound ?? ""}
          onchange={(e) => {
            const pick = e.currentTarget.value;
            // Default inherits — preview what the account will really use.
            void previewNotificationSound(pick === "" ? settings.sound : pick);
            void saveAccount(account, account.notifyEnabled, pick === "" ? null : pick);
          }}
        >
          <option value="">Default</option>
          <option value="none">None</option>
          {#each SOUNDS as sound (sound)}
            <option value={sound}>{sound}</option>
          {/each}
        </select>
      </div>
    {:else}
      <p class="empty">No accounts yet.</p>
    {/each}
  </fieldset>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}
</section>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    gap: 18px;
    padding: 20px 26px;
    overflow-y: auto;
  }

  /* White grouped boxes on the settings canvas, like the other tabs. */
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

  .explain {
    margin: 6px 0 2px;
    font-size: 11.5px;
    color: var(--text-secondary);
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

  select {
    min-width: 11rem;
    padding: 3px 8px;
    border: 1px solid var(--border-chrome);
    border-radius: 6px;
    background: var(--bg-window);
    font: inherit;
    font-size: 12.5px;
    font-weight: 400;
  }

  .account-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 5px 0;
    font-size: 13px;
    font-weight: 500;
  }

  .account-name {
    width: 10rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .empty {
    margin: 4px 0;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .error {
    margin: 0;
    font-size: 12px;
    color: #d9302c;
  }
</style>
