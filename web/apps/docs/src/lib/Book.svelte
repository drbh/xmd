<script>
  // The book view: every feature explained beside a live, editable example.
  // Examples run on the app's workspace under their own URIs and are never
  // persisted; reloading restores the originals.
  import { onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import Editor from "./Editor.svelte";
  import { parts } from "./chapters.js";
  import { examples, bookUri } from "./examples.js";
  let { workspace, theme, onToggleTheme, onHome, onError } = $props();
  let loaded = $state(false), problems = $state({}), current = $state(""), main;
  const slug = s => s.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/(^-|-$)/g, "");
  const blocks = parts.flatMap(part => part.chapters.flatMap(c => [c.file, c.companion].filter(Boolean)));
  onMount(() => {
    let observer;
    (async () => {
      for (const file of blocks) await workspace.setDocument(bookUri(file), examples[file] ?? `<!-- ${file} is missing -->\n`);
      loaded = true;
      // Follow the reader through the contents, and honour a chapter in the URL.
      observer = new IntersectionObserver(entries => { for (const e of entries) if (e.isIntersecting) current = e.target.id; }, { root: main, rootMargin: "-10% 0px -80% 0px" });
      for (const section of main.querySelectorAll("section[id]")) observer.observe(section);
      const target = decodeURIComponent(location.hash.replace(/^#\/book\/?/, ""));
      if (target) { await document.fonts?.ready; setTimeout(() => document.getElementById(target)?.scrollIntoView(), 150); }
    })().catch(onError);
    return () => observer?.disconnect();
  });
  function jump(id) { document.getElementById(id)?.scrollIntoView({ behavior: "smooth" }); history.replaceState(null, "", `#/book/${id}`); }
</script>

<div class="app book">
  <header class="chrome">
    <div class="titlebar">
      <button type="button" class="logo" title="Back to documents" aria-label="Documents home" onclick={onHome}><Icon name="doc" size={26} /></button>
      <div class="title-block book-title"><span class="brand">The WTF Book</span><span class="subtitle">Every feature, as a live note</span></div>
      <div class="title-actions">
        <button type="button" class="button" onclick={onHome}>Open the editor</button>
        <button type="button" class="tool theme-toggle" title={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"} aria-label="Toggle theme" onclick={onToggleTheme}><Icon name={theme === "dark" ? "sun" : "moon"} /></button>
      </div>
    </div>
  </header>
  <div class="workspace">
    <nav class="sidebar toc" aria-label="Contents">
      <h2>Contents</h2>
      <ol>
        {#each parts as part, p}
          <li>
            <button type="button" class="row part-link" aria-current={current === `part-${p + 1}` ? "location" : undefined} onclick={() => jump(`part-${p + 1}`)}>{part.title}</button>
            <ol>{#each part.chapters as chapter, c}<li><button type="button" class="row" aria-current={current === slug(chapter.title) ? "location" : undefined} onclick={() => jump(slug(chapter.title))}>{p + 1}.{c + 1} {chapter.title}</button></li>{/each}</ol>
          </li>
        {/each}
      </ol>
    </nav>
    <main class="canvas" bind:this={main}>
      <article class="page prose">
        <header class="title">
          <h1>The WTF Book</h1>
          <p class="lead">Plain text that calculates, counts, plans, and remembers. Every feature below is a live note on the same engine as the editor: edit it, hover a name, tick a box.</p>
        </header>
        {#each parts as part, p}
          <section class="part" id={`part-${p + 1}`}>
            <p class="kicker">Part {p + 1}</p>
            <h1>{part.title}</h1>
            <p class="lead">{@html part.intro}</p>
            {#each part.chapters as chapter, c}
              <section class="chapter" id={slug(chapter.title)}>
                <h2><span class="number">{p + 1}.{c + 1}</span> {chapter.title}</h2>
                {#each chapter.prose as paragraph}<p>{@html paragraph}</p>{/each}
                {#each [chapter.file, chapter.companion].filter(Boolean) as file (file)}
                  <figure class="wtf-block" data-file={file}>
                    <figcaption><span>examples/{file}</span></figcaption>
                    {#if loaded}
                      <Editor {workspace} uri={bookUri(file)} text={examples[file] ?? ""} frame="block" onSnapshot={(s, list) => (problems = { ...problems, [file]: list })} {onError} />
                    {/if}
                    {#if problems[file]?.length}
                      <ul class="problems">{#each problems[file] as d}<li class={d.severity === 2 ? "warn" : "error"}>Line {d.range.start.line + 1}: {d.message}</li>{/each}</ul>
                    {/if}
                  </figure>
                {/each}
              </section>
            {/each}
          </section>
        {/each}
        <footer><p>The blocks are the files in <code>examples/</code>.</p></footer>
      </article>
    </main>
  </div>
</div>
