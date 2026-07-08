<script lang="ts">
  import type { NewAccount } from "./types";

  let {
    onSubmit,
    onCancel,
  }: {
    onSubmit: (account: NewAccount, password: string) => void;
    onCancel: () => void;
  } = $props();

  let name = $state("");
  let email = $state("");
  let imapHost = $state("");
  let imapPort = $state(993);
  let smtpHost = $state("");
  let smtpPort = $state(587);
  let username = $state("");
  let password = $state("");

  function submit(event: SubmitEvent) {
    event.preventDefault();
    onSubmit(
      { name, email, imapHost, imapPort, smtpHost, smtpPort, username },
      password,
    );
  }
</script>

<form onsubmit={submit}>
  <h2>Add account</h2>

  <label>Name <input bind:value={name} required /></label>
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
    <button type="submit">Add</button>
  </footer>
</form>

<style>
  form {
    display: flex;
    flex-direction: column;
    gap: 0.625rem;
    width: 20rem;
    padding: 1rem 1.25rem 1.25rem;
    border: 1px solid #e5e5e5;
    border-radius: 0.5rem;
    background: #fff;
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
    color: #444;
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
    border: 1px solid #d4d4d4;
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
    border: 1px solid #d4d4d4;
    border-radius: 0.375rem;
    background: #fff;
    font: inherit;
    cursor: pointer;
  }

  button[type="submit"] {
    background: #1a1a1a;
    border-color: #1a1a1a;
    color: #fff;
  }
</style>
