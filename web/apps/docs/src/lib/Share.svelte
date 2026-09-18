<script>
  // Manage who can open a document. Only the owner can change the list; the
  // backend enforces that, this just reflects it.
  import Dialog from "./Dialog.svelte";
  let { acl, name, onClose } = $props();
  let state = $state(null), email = $state(""), role = $state("editor"), error = $state(""), busy = $state(false);
  async function load() { try { state = await acl.list(); } catch (e) { error = e.message; } }
  $effect(() => { load(); });
  async function add() {
    if (!email.trim() || busy) return;
    busy = true; error = "";
    try { await acl.add(email.trim(), role); email = ""; await load(); } catch (e) { error = e.message; } finally { busy = false; }
  }
  async function remove(target) {
    busy = true; error = "";
    try { await acl.remove(target); await load(); } catch (e) { error = e.message; } finally { busy = false; }
  }
</script>

<Dialog title={`Share “${name}”`} {onClose}>
  {#if !state}<p>{error || "Loading…"}</p>
  {:else}
    {#if state.role === "owner"}
      <form class="share-add" onsubmit={e => { e.preventDefault(); add(); }}>
        <input type="email" placeholder="Add people by email" aria-label="Email" bind:value={email} required>
        <select aria-label="Role" bind:value={role}><option value="editor">Editor</option><option value="viewer">Viewer</option></select>
        <button type="submit" class="button primary" disabled={busy || !email.trim()}>Share</button>
      </form>
    {/if}
    {#if error}<p class="error-text">{error}</p>{/if}
    <ul class="people">
      <li><span class="who">{state.owner?.email}</span><span class="role">Owner</span></li>
      {#each state.entries as entry (entry.email)}
        <li><span class="who">{entry.email}</span><span class="role">{entry.role}</span>{#if state.role === "owner"}<button type="button" class="link" onclick={() => remove(entry.email)}>Remove</button>{/if}</li>
      {/each}
      {#each state.invites as invite (invite.email)}
        <li><span class="who">{invite.email}</span><span class="role">{invite.role} · invited</span>{#if state.role === "owner"}<button type="button" class="link" onclick={() => remove(invite.email)}>Remove</button>{/if}</li>
      {/each}
    </ul>
    <p class="muted">People you add sign in with the same account provider. Anyone who hasn't signed in yet gets access the first time they do.</p>
  {/if}
</Dialog>
