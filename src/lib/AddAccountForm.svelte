<script lang="ts">
  import type { NewAccount } from "./types";

  let {
    onSubmit,
    onCancel,
    onSignInWithGoogle,
    onCancelSignIn,
  }: {
    onSubmit: (account: NewAccount, password: string) => Promise<void> | void;
    onCancel: () => void;
    /** Absent when this build can't sign in with Google — no button then. */
    onSignInWithGoogle?: (name: string) => Promise<void>;
    onCancelSignIn?: () => void;
  } = $props();

  let submitting = $state(false);
  let signingIn = $state(false);

  let name = $state("");
  let email = $state("");
  let imapHost = $state("");
  let imapPort = $state(993);
  let smtpHost = $state("");
  let smtpPort = $state(587);
  let username = $state("");
  let password = $state("");

  async function signInWithGoogle() {
    if (!onSignInWithGoogle) return;
    signingIn = true;
    try {
      // why the name only: the address and servers come from the sign-in;
      // an empty name makes the backend use the address.
      await onSignInWithGoogle(name);
    } finally {
      signingIn = false;
    }
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    submitting = true;
    try {
      await onSubmit(
        { name, email, imapHost, imapPort, smtpHost, smtpPort, username },
        password,
      );
    } finally {
      submitting = false;
    }
  }
</script>

<form onsubmit={submit}>
  <h2>Add account</h2>

  <label>Name <input bind:value={name} required /></label>
  {#if onSignInWithGoogle}
    <!-- why type="button": Google needs no other field, so it must not
         trigger the form's required-field validation. -->
    <button
      type="button"
      class="google"
      disabled={signingIn || submitting}
      onclick={signInWithGoogle}
    >
      {signingIn ? "Waiting for Google…" : "Sign in with Google"}
    </button>
    {#if signingIn}
      <p class="waiting">
        Finish signing in in your browser, then come back here.
        <button type="button" class="link" onclick={() => onCancelSignIn?.()}>
          Cancel sign-in
        </button>
      </p>
    {/if}
    <p class="divider">or set up any other account over IMAP</p>
  {/if}
  <label>Email <input type="email" bind:value={email} required /></label>
  <div class="pair">
    <label>IMAP host <input bind:value={imapHost} required /></label>
    <label
      >IMAP port <input type="number" bind:value={imapPort} required /></label
    >
  </div>
  <div class="pair">
    <label>SMTP host <input bind:value={smtpHost} required /></label>
    <label
      >SMTP port <input type="number" bind:value={smtpPort} required /></label
    >
  </div>
  <label>Username <input bind:value={username} required /></label>
  <label
    >Password <input type="password" bind:value={password} required /></label
  >

  <footer>
    <button type="button" onclick={onCancel}>Cancel</button>
    <!-- why: saving is gated on a live IMAP+SMTP check (test_connection),
         hence the label — the button pins the invariant into the UI. -->
    <button type="submit" disabled={submitting || signingIn}>
      {submitting ? "Verifying…" : "Verify & Save"}
    </button>
  </footer>
</form>

<style>
  /* why: no card chrome — the form renders inside the settings detail pane,
     which already provides padding and a bounded, scrollable container. */
  form {
    display: flex;
    flex-direction: column;
    gap: 0.625rem;
    width: 100%;
    max-width: 22rem;
  }

  h2 {
    margin: 0 0 0.25rem;
    font-size: 1rem;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    flex: 1;
    font-size: 0.75rem;
    color: var(--text-secondary);
  }

  .pair {
    display: flex;
    gap: 0.5rem;
  }

  .pair label:last-child {
    flex: 0 0 5rem;
  }

  input {
    padding: 0.375rem 0.5rem;
    border: 1px solid var(--card-border-strong);
    border-radius: 0.375rem;
    font: inherit;
    color: inherit;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.375rem;
  }

  button {
    padding: 0.375rem 0.875rem;
    border: 1px solid var(--card-border-strong);
    border-radius: 0.375rem;
    background: var(--bg-card);
    font: inherit;
    cursor: pointer;
  }

  .google {
    align-self: flex-start;
    font-weight: 600;
  }

  .waiting,
  .divider {
    margin: 0;
    font-size: 0.75rem;
    color: var(--text-secondary);
  }

  .divider {
    margin-top: 0.25rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--card-border-strong);
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font-size: inherit;
  }

  /* why accent, not the old black: the other settings panes fill their
     primary button with the accent, and black has no dark-mode reading. */
  button[type="submit"] {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--on-accent);
  }
</style>
