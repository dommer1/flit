<script lang="ts">
  import { onMount } from "svelte";
  import {
    getNotificationSettings,
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
        onchange={(e) =>
          void save({ ...settings, sound: e.currentTarget.value })}
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
          onchange={(e) =>
            void saveAccount(
              account,
              account.notifyEnabled,
              e.currentTarget.value === "" ? null : e.currentTarget.value,
            )}
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
    gap: 1rem;
    padding: 1rem 1.25rem;
    overflow-y: auto;
  }

  fieldset {
    margin: 0;
    padding: 0;
    border: none;
  }

  legend {
    padding: 0;
    font-weight: 600;
  }

  .explain {
    margin: 0.375rem 0 0.5rem;
    max-width: 34rem;
    font-size: 0.8125rem;
    color: var(--text-secondary, #6e6e73);
  }

  .setting {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.25rem 0;
    font-size: 13px;
  }

  .setting label {
    width: 8.5rem;
  }

  select {
    min-width: 11rem;
    font: inherit;
    font-size: 13px;
  }

  .account-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.25rem 0;
    font-size: 13px;
  }

  .account-name {
    width: 8.5rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .empty {
    margin: 0.25rem 0;
    font-size: 13px;
    color: var(--text-secondary, #6e6e73);
  }

  .error {
    margin: 0;
    font-size: 12px;
    color: #d9302c;
  }
</style>
