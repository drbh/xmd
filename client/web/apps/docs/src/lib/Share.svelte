<script>
  // Manage who can open a document. Only the owner can change the list; the
  // backend enforces that, this just reflects it.
  import Dialog from "./Dialog.svelte";
  import Icon from "./Icon.svelte";
  import { colorFor } from "./store.js";
  let { acl, link = null, name, kind = "document", onClose } = $props();
  let state = $state(null), email = $state(""), role = $state("editor"), error = $state(""), busy = $state(false), copied = $state(false);
  // "Anyone with the link": one view-only link per document.
  let linkState = $state(null), linkCopied = $state(false);
  const linkUrl = token => `${location.origin}${location.pathname}#/s/${token}`;
  async function loadLink() { if (!link) return; try { linkState = await link.get(); } catch { linkState = null; } }
  $effect(() => { loadLink(); });
  async function toggleLink(on) {
    busy = true; error = "";
    try { linkState = on ? await link.enable() : await link.disable(); } catch (e) { error = e.message; } finally { busy = false; }
  }
  async function copyShareLink() { try { await navigator.clipboard.writeText(linkUrl(linkState.token)); linkCopied = true; setTimeout(() => (linkCopied = false), 2000); } catch { /* select it by hand */ } }
  async function copyLink() {
    try { await navigator.clipboard.writeText(location.href.replace(/\?test/, "")); copied = true; setTimeout(() => (copied = false), 2000); } catch { error = "Copy the address bar link instead."; }
  }
  async function setRole(target, next) {
    busy = true; error = "";
    try { await acl.add(target, next); await load(); } catch (e) { error = e.message; } finally { busy = false; }
  }
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

<Dialog title={`Share ${kind === "folder" ? "folder " : ""}“${name}”`} {onClose}>
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
      <li><span class="avatar tiny" style={`background:${colorFor(state.owner?.email)}`}>{(state.owner?.email || "?")[0].toUpperCase()}</span><span class="who">{state.owner?.email}</span><span class="role">Owner</span></li>
      {#each [...state.entries.map(e => ({ ...e, invited: false })), ...state.invites.map(i => ({ ...i, invited: true }))] as entry (entry.email)}
        <li>
          <span class="avatar tiny" style={`background:${colorFor(entry.email)}`}>{entry.email[0].toUpperCase()}</span>
          <span class="who">{entry.email}{#if entry.invited}<span class="doc-tag">Invited</span>{/if}</span>
          {#if state.role === "owner"}
            <select class="select role-select" aria-label={`Role for ${entry.email}`} value={entry.role} disabled={busy} onchange={e => setRole(entry.email, e.target.value)}><option value="editor">Editor</option><option value="viewer">Viewer</option></select>
            <button type="button" class="link" disabled={busy} onclick={() => remove(entry.email)}>Remove</button>
          {:else}<span class="role">{entry.role}</span>{/if}
        </li>
      {/each}
    </ul>
    {#if link && linkState}
      <div class="link-share">
        <div class="link-row">
          <span class="link-icon"><Icon name="link" /></span>
          <div class="link-text"><strong>Anyone with the link</strong><span class="muted">{linkState.enabled ? "can view this document, no sign-in needed" : "Off · only the people above can open it"}</span></div>
          {#if state.role === "owner"}<button type="button" class="button" disabled={busy} onclick={() => toggleLink(!linkState.enabled)}>{linkState.enabled ? "Turn off" : "Turn on"}</button>{/if}
        </div>
        {#if linkState.enabled && linkState.token}
          <div class="link-url"><code>{linkUrl(linkState.token)}</code><button type="button" class="button primary" onclick={copyShareLink}>{linkCopied ? "Copied" : "Copy link"}</button></div>
        {/if}
      </div>
    {/if}
    {#if !linkState?.enabled}
      <div class="share-foot">
        {#if kind === "document"}<button type="button" class="button" onclick={copyLink}><Icon name="link" /> {copied ? "Link copied" : "Copy link"}</button>{/if}
        <span class="muted">{kind === "folder" ? "People here can open every document in the folder, now and later." : "Only people listed here can open it."} Anyone who hasn't signed in yet gets access the first time they do.</span>
      </div>
    {/if}
  {/if}
</Dialog>
