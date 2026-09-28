<script>
  // API keys for the sync plugin. A new key is shown once; only its hash is kept.
  import Dialog from "./Dialog.svelte";
  import Icon from "./Icon.svelte";
  import { relativeTime } from "./store.js";
  import { EXTENSION } from "@xmd/web";
  let { keys, site, onClose } = $props();
  let list = $state(null), name = $state(""), fresh = $state(null), error = $state(""), busy = $state(false), copied = $state(false);
  async function load() { try { list = await keys.list(); } catch (e) { error = e.message; } }
  $effect(() => { load(); });
  async function create() {
    busy = true; error = "";
    try { fresh = await keys.create(name.trim() || "Sync key"); name = ""; await load(); } catch (e) { error = e.message; } finally { busy = false; }
  }
  async function revoke(k) {
    if (!confirm(`Revoke "${k.name}"? Anything using it stops syncing.`)) return;
    try { await keys.revoke(k.id); if (fresh?.id === k.id) fresh = null; await load(); } catch (e) { error = e.message; }
  }
  async function copy(text) { try { await navigator.clipboard.writeText(text); copied = true; setTimeout(() => (copied = false), 2000); } catch { /* select it by hand */ } }
</script>

<Dialog title="API keys" {onClose} wide>
  <p>A key lets the command line sync a folder of <code>.{EXTENSION}</code> files with this account: <code>xmd run sync ./notes --url {site} --folder Notes</code>. Keys can read and write your documents and folders, not share them.</p>
  {#if fresh}
    <div class="fresh-key">
      <p><strong>Your new key.</strong> Copy it now; it is not shown again.</p>
      <code class="key">{fresh.key}</code>
      <div class="share-foot"><button type="button" class="button primary" onclick={() => copy(fresh.key)}><Icon name="link" /> {copied ? "Copied" : "Copy key"}</button><span class="muted">Then: <code>export XMD_API_KEY={fresh.key.slice(0, 8)}…</code></span></div>
    </div>
  {/if}
  <form class="share-add" onsubmit={e => { e.preventDefault(); create(); }}>
    <input type="text" placeholder="What is this key for? (e.g. laptop)" aria-label="Key name" bind:value={name}>
    <button type="submit" class="button primary" disabled={busy}>Create key</button>
  </form>
  {#if error}<p class="error-text">{error}</p>{/if}
  {#if list}
    {#if list.length}
      <ul class="people">
        {#each list as k (k.id)}
          <li><span class="who">{k.name}</span><span class="role">{k.lastUsed ? `used ${relativeTime(k.lastUsed)}` : "never used"} · created {relativeTime(k.created)}</span><button type="button" class="link" onclick={() => revoke(k)}>Revoke</button></li>
        {/each}
      </ul>
    {:else}<p class="muted">No keys yet.</p>{/if}
  {/if}
</Dialog>
