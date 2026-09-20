<script>
  // The reference is one page: the syntax table, then the engine's own
  // reference, every snippet drawn by the engine the way the editor shows a
  // note. Nothing here is written by hand except the sample note, so the page
  // cannot fall behind the language.
  import { onMount, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import Try from "./Try.svelte";
  import { SAMPLE, SAMPLE_URI, SYNTAX_URI, loadReference, createSnippetRenderer, renderSyntaxes } from "./reference.js";
  let { workspace, theme, onToggleTheme, onHome, onError } = $props();
  let loaded = $state(false), live = $state(false), sections = $state.raw([]), syntaxes = $state.raw([]);
  let current = $state("start"), tocOpen = $state(false), main;
  // One open Try per section, so the page never hosts dozens of editors.
  let open = $state({});
  // The rendered snippets, by entry id, as they arrive from the engine.
  let html = $state.raw({}), drawn = $state(""), renderer;
  const contents = $derived([{ id: "start", title: "Getting started", groups: [] }, { id: "syntax", title: "Syntax", groups: [] }, ...sections].map(s => ({
    id: s.id, title: s.title, groups: s.id === "functions" ? s.groups.map(g => ({ id: g.id, title: g.title })) : [],
  })));
  function toggle(section, entry) {
    open = { ...open, [section]: open[section] === entry.id ? null : entry.id };
  }
  // `#/reference/<name>` scrolls to an entry, on arrival and whenever the
  // address changes. The blocks above it grow as they render, so hold it in
  // view until the page has settled.
  function locate() {
    const target = decodeURIComponent(location.hash.replace(/^#\/reference\/?/, ""));
    if (!target) return;
    for (const delay of [0, 150, 600, 1200]) setTimeout(() => document.getElementById(target)?.scrollIntoView({ block: "center" }), delay);
  }
  // A snippet the reader can see renders before the ones above it.
  function watch(node, id) {
    const observer = new IntersectionObserver(entries => { for (const e of entries) if (e.isIntersecting) renderer?.prioritise(id); }, { root: main, rootMargin: "200px 0px" });
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }
  onMount(() => {
    let observer;
    renderer = createSnippetRenderer(workspace, {
      onRender: (id, fragment) => { html = { ...html, [id]: fragment }; },
      onDone: ({ total, ms }) => { drawn = `${total} snippets in ${ms} ms`; console.info(`reference: rendered ${total} snippets in ${ms} ms`); },
    });
    (async () => {
      await workspace.setDocument(SAMPLE_URI, SAMPLE);
      const reference = await loadReference(workspace);
      sections = reference.sections;
      syntaxes = reference.syntaxes;
      live = reference.live;
      loaded = true;
      // The syntax table first, as one note; then the rows in page order,
      // the first batch now and the rest as the browser idles or the reader
      // scrolls to them.
      html = { ...html, ...(await renderSyntaxes(workspace, syntaxes)) };
      const snippets = [];
      for (const section of sections) for (const group of section.groups) for (const entry of group.rows) if (entry.try && entry.kind !== "query") snippets.push({ id: entry.id, text: entry.try });
      renderer.enqueue(snippets);
      await tick();
      // Follow the reader through the contents, and honour an entry in the URL.
      observer = new IntersectionObserver(entries => { for (const e of entries) if (e.isIntersecting) current = e.target.id; }, { root: main, rootMargin: "-10% 0px -80% 0px" });
      for (const { id } of contents.flatMap(s => (s.groups.length ? s.groups : [s]))) { const el = document.getElementById(id); if (el) observer.observe(el); }
      await document.fonts?.ready;
      locate();
    })().catch(onError);
    return () => { observer?.disconnect(); renderer?.dispose(); Promise.resolve(workspace.removeDocument(SYNTAX_URI)).catch(() => {}); };
  });
  function jump(id) {
    tocOpen = false;
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth" });
    history.replaceState(null, "", `#/reference/${id}`);
  }
</script>

<svelte:window onhashchange={locate} />

{#snippet snippet(entry)}
  {#if entry.kind === "query"}
    <pre class="snippet query"><code>{entry.try}</code></pre>
  {:else if html[entry.id] !== undefined}
    <pre class="snippet wtf" data-layout="source"><code>{@html html[entry.id]}</code></pre>
  {:else}
    <pre class="snippet wtf pending" data-layout="source" use:watch={entry.id}><code>{entry.try}</code></pre>
  {/if}
{/snippet}

{#snippet rows(section, group)}
  <table class="ref" class:compact={group.compact} class:plain={!group.rows.some(r => r.try)}>
    <tbody>
      {#each group.rows as entry (entry.id)}
        <tr class="ref-row" id={entry.id} data-name={entry.name} data-tier={entry.tier}>
          <td class="sig">
            <code class="wtf sig">{@html entry.signature}</code>
            {#if entry.tier !== "note" && entry.tier !== "object"}<span class="tier">{entry.tier}</span>{/if}
          </td>
          <td class="doc">{entry.documentation}</td>
          <td class="run">{#if entry.try && open[section.id] !== entry.id}{@render snippet(entry)}{/if}</td>
          <td class="act">{#if entry.try}<button type="button" class="edit" onclick={() => toggle(section.id, entry)}>{open[section.id] === entry.id ? "close" : "edit"}</button>{/if}</td>
        </tr>
        {#if entry.fields?.length}
          <tr class="fields-row"><td colspan="4"><span class="fields">{#each entry.fields as field}<code>{field}</code>{/each}</span></td></tr>
        {/if}
        {#if open[section.id] === entry.id}
          <tr class="try-row"><td colspan="4">
            <Try {workspace} uri={entry.uri} text={entry.try} kind={entry.kind === "query" ? "query" : "note"} {onError} />
          </td></tr>
        {/if}
      {/each}
    </tbody>
  </table>
{/snippet}

<div class="app reference-view">
  <header class="chrome">
    <div class="titlebar">
      <button type="button" class="logo" title="Back to documents" aria-label="Documents home" onclick={onHome}><Icon name="doc" size={26} /></button>
      <div class="title-block reference-title"><span class="brand">WTF Reference</span><span class="subtitle">the language, complete</span></div>
      <div class="title-actions">
        <button type="button" class="button" onclick={onHome}>Open the editor</button>
        <button type="button" class="tool theme-toggle" title={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"} aria-label="Toggle theme" onclick={onToggleTheme}><Icon name={theme === "dark" ? "sun" : "moon"} /></button>
      </div>
    </div>
  </header>
  <div class="workspace">
    <nav class="sidebar toc" class:mobile-open={tocOpen} aria-label="Contents">
      <h2>Contents</h2>
      <ol>
        {#each contents as section}
          <li>
            <button type="button" class="row part-link" aria-current={current === section.id ? "location" : undefined} onclick={() => jump(section.id)}>{section.title}</button>
            {#if section.groups.length}
              <ol>{#each section.groups as group}<li><button type="button" class="row" aria-current={current === group.id ? "location" : undefined} onclick={() => jump(group.id)}>{group.title}</button></li>{/each}</ol>
            {/if}
          </li>
        {/each}
      </ol>
    </nav>
    <main class="canvas" bind:this={main} data-snippets={drawn}>
      <article class="page prose">
        <header class="title">
          <h1>Reference</h1>
          <p class="lead">Every function, attribute, type, collection and library the engine knows, each with a snippet and its result. Find in page reaches everything.</p>
        </header>
        <section class="start" id="start">
          <h2>Getting started</h2>
          <p>One binary is the command line and the language server. One command installs it; no Rust toolchain needed.</p>
          <pre class="setup"><code>curl -fsSL https://github.com/drbh/jot/releases/latest/download/install.sh | sh
wtf --version</code></pre>
          <p>Or grab a binary from the <a href="https://github.com/drbh/jot/releases">releases page</a>. Then tell your editor. Each setup finds <code>wtf</code> on <code>PATH</code>; a setting overrides that if it lives elsewhere.</p>
          <dl class="editors">
            <dt>VS Code</dt>
            <dd>Install the <code>.vsix</code> from the releases page (Extensions view &rsaquo; &hellip; &rsaquo; Install from VSIX); it downloads the server itself. Open a <code>.wtf</code> file. The <code>wtf.serverPath</code> setting overrides the binary.</dd>
            <dt>Zed</dt>
            <dd>Unpack <code>wtf-zed.tar.gz</code> from the releases page, then Extensions &rsaquo; Install Dev Extension &rsaquo; that folder; it downloads the server itself. <code>lsp.wtf.binary.path</code> in settings overrides the binary.</dd>
            <dt>Neovim 0.11+</dt>
            <dd>Install the binary with the one-liner, then add <code>dofile("/path/to/jot/client/ide/neovim/wtf.lua")</code> to <code>init.lua</code>. <code>vim.g.wtf_server_path</code> overrides the binary.</dd>
            <dt>Helix</dt>
            <dd>Install the binary with the one-liner, then merge <code>client/ide/helix/languages.toml</code> into <code>~/.config/helix/languages.toml</code>; run <code>hx --grammar fetch</code> if Markdown highlighting is missing.</dd>
          </dl>
          <p>Build from source instead: <code>cargo install --path lang</code> from a checkout.</p>
          <p>No editor at all: <code>wtf trip.wtf 'total'</code> from the shell, or write in this app and the note stays in the browser.</p>
        </section>
        <section class="syntax" id="syntax">
          <h2>Syntax</h2>
          <table class="syntaxes">
            <tbody>
              {#each syntaxes as entry (entry.id)}
                <tr><td class="sig">{@render snippet({ id: entry.id, try: entry.syntax })}</td><td class="doc">{entry.meaning}</td></tr>
              {/each}
            </tbody>
          </table>
        </section>
        <div class="reference">
          {#if !live && loaded}<p class="offline">offline reference</p>{/if}
          {#each sections as section (section.id)}
            <section class="ref-section" id={section.id}>
              <h2>{section.title}</h2>
              {#each section.groups as group (group.id)}
                <section class="ref-group" id={group.id}>
                  {#if group.title !== section.title}<h3>{group.title}{#if group.kind}<span class="kind">{group.kind}</span>{/if}</h3>{/if}
                  {#if group.documentation}<p class="group-doc">{group.documentation}</p>{/if}
                  {@render rows(section, group)}
                </section>
              {/each}
            </section>
          {/each}
          {#if !loaded}<p class="empty">Reading the reference…</p>{/if}
        </div>
      </article>
    </main>
  </div>
  <button type="button" class="chip outline-toggle" aria-label="Contents" onclick={() => (tocOpen = !tocOpen)}><Icon name="outline" size={16} /></button>
</div>
