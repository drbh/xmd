<script>
  // A developer-console drawer: type a query in the note's functional language
  // and see the engine's rows. Read-only; it never edits the document.
  import { tick } from "svelte";
  import Icon from "./Icon.svelte";
  import { complete, apply, learnFields } from "./completions.js";
  let { rpc, uri, names = [], onClose, height = $bindable(260) } = $props();
  let entries = $state.raw([]), input = $state(""), scope = $state("document"), busy = $state(false);
  let menu = $state(null), selected = $state(0), fields = $state.raw({});
  let history = [], cursor = -1, field, log;
  const EXAMPLES = ["values", "tasks | where !done | select {title, due}", "sum(tasks.estimate)", "{open: length(filter(tasks, fn(t) => !t.done)), problems: length(diagnostics)}", "graph.edges"];
  $effect(() => { field?.focus(); });
  $effect(() => { entries; const el = log; if (el) requestAnimationFrame(() => { el.scrollTop = el.scrollHeight; }); });
  // Field names come from the current document; relearn when it changes.
  $effect(() => { const target = uri; learnFields(rpc, target).then(f => { if (target === uri) { fields = f; if (document.activeElement === field && input) refresh(); } }); });

  const SYMBOL = { USD: "$", EUR: "€", GBP: "£", JPY: "¥" };
  // Typed values from the engine become readable text; everything else stays structured.
  function scalar(v) {
    if (v === null || v === undefined) return "null";
    if (typeof v !== "object") return typeof v === "string" ? v : String(v);
    if (Array.isArray(v)) return null;
    switch (v.type) {
      case "money": return `${SYMBOL[v.currency] ?? v.currency + " "}${Number(v.amount).toLocaleString(undefined, { maximumFractionDigits: 2 })}`;
      case "date": case "time": case "datetime": return String(v.value);
      case "duration": { const s = v.seconds ?? 0; const h = Math.floor(s / 3600), m = Math.floor(s % 3600 / 60), r = s % 60; return [h && `${h}h`, m && `${m}m`, (r || !s) && `${r}s`].filter(Boolean).join(" "); }
      case "ratio": case "percent": return v.value !== undefined ? `${v.value}%` : null;
      default: return null;
    }
  }
  const pretty = v => JSON.stringify(v, (k, x) => { const s = x && typeof x === "object" && !Array.isArray(x) && "type" in x ? scalar(x) : null; return s ?? x; }, 2);
  function present(rows) {
    if (!Array.isArray(rows)) return { kind: "text", text: pretty(rows) };
    if (!rows.length) return { kind: "text", text: "(no rows)" };
    if (rows.every(r => scalar(r) !== null)) return { kind: "list", items: rows.map(scalar) };
    const flat = rows.every(r => r && typeof r === "object" && !Array.isArray(r) && Object.values(r).every(v => scalar(v) !== null || Array.isArray(v)));
    if (flat) {
      const columns = [...new Set(rows.flatMap(r => Object.keys(r)))];
      return { kind: "table", columns, rows: rows.map(r => columns.map(c => c in r ? (scalar(r[c]) ?? pretty(r[c])) : "")) };
    }
    return { kind: "text", text: pretty(rows) };
  }
  // Results teach field names too: `tasks | select {title}` learns nothing new,
  // but `get(tasks, 0)` or a custom record reveals keys for the next query.
  function learn(query, rows) {
    const owner = /^\s*([a-z_]+)\b/.exec(query)?.[1];
    const sample = Array.isArray(rows) ? rows[0] : rows;
    if (owner && sample && typeof sample === "object" && !Array.isArray(sample) && !fields[owner]) fields = { ...fields, [owner]: Object.keys(sample).sort() };
  }
  async function run(query = input.trim()) {
    if (!query || busy) return;
    history = [query, ...history.filter(h => h !== query)].slice(0, 100); cursor = -1;
    input = ""; menu = null; busy = true;
    const entry = { id: crypto.randomUUID(), query, scope, result: null, error: null };
    entries = [...entries, entry];
    try {
      const response = await rpc("query", { query, uri: scope === "document" ? uri : null });
      entry.result = present(response.rows); entry.count = Array.isArray(response.rows) ? response.rows.length : 1;
      learn(query, response.rows);
    } catch (e) { entry.error = String(e.message || e).replace(/^Error:\s*/, ""); }
    finally { busy = false; entries = entries.map(e => e.id === entry.id ? { ...entry } : e); }
  }
  function refresh() {
    const result = field ? complete(input, field.selectionStart ?? input.length, { fields, names }) : null;
    menu = result?.items.length ? result : null;
    selected = 0;
  }
  function accept(item = menu?.items[selected]) {
    if (!menu || !item) return;
    const next = apply(input, field.selectionStart ?? input.length, menu.start, item);
    input = next.text; menu = null;
    // Place the caret only once the new value is in the DOM.
    tick().then(() => { field.setSelectionRange(next.caret, next.caret); field.focus(); });
  }
  function keydown(event) {
    if (menu) {
      if (event.key === "ArrowDown") { event.preventDefault(); selected = (selected + 1) % menu.items.length; return; }
      if (event.key === "ArrowUp") { event.preventDefault(); selected = (selected - 1 + menu.items.length) % menu.items.length; return; }
      if (event.key === "Tab" || event.key === "Enter") { event.preventDefault(); accept(); return; }
      if (event.key === "Escape") { event.preventDefault(); menu = null; return; }
    }
    if (event.key === "Enter") { event.preventDefault(); run(); }
    else if (event.key === "Tab") { event.preventDefault(); refresh(); }
    else if (event.key === "ArrowUp" && history.length) { event.preventDefault(); cursor = Math.min(cursor + 1, history.length - 1); input = history[cursor]; }
    else if (event.key === "ArrowDown") { event.preventDefault(); cursor = Math.max(cursor - 1, -1); input = cursor === -1 ? "" : history[cursor]; }
    else if (event.key === "Escape") { event.preventDefault(); onClose(); }
    else if (event.key === "l" && event.ctrlKey) { event.preventDefault(); entries = []; }
  }
  function resize(event) {
    const startY = event.clientY, start = height;
    const move = e => (height = Math.max(120, Math.min(innerHeight * 0.8, start + startY - e.clientY)));
    const stop = () => { removeEventListener("mousemove", move); removeEventListener("mouseup", stop); };
    addEventListener("mousemove", move); addEventListener("mouseup", stop);
    event.preventDefault();
  }
</script>

<section class="console" style={`height:${height}px`} aria-label="Query console" data-fields={Object.keys(fields).length ? "ready" : "loading"}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="grip" onmousedown={resize}></div>
  <header>
    <span class="console-title">Console</span>
    <div class="scope" role="radiogroup" aria-label="Query scope">
      <button type="button" role="radio" aria-checked={scope === "document"} class:on={scope === "document"} onclick={() => (scope = "document")}>This document</button>
      <button type="button" role="radio" aria-checked={scope === "workspace"} class:on={scope === "workspace"} onclick={() => (scope = "workspace")}>All documents</button>
    </div>
    <span class="gap"></span>
    <button type="button" class="tool" title="Clear (Ctrl+L)" aria-label="Clear console" onclick={() => (entries = [])}><Icon name="close" size={16} /></button>
    <button type="button" class="tool" title="Close console (Esc)" aria-label="Close console" onclick={onClose}><Icon name="down" size={16} /></button>
  </header>
  <div class="log" bind:this={log}>
    {#if !entries.length}
      <p class="hint">Query the resolved document with its functional language. Type to see collections, fields, and functions; Tab completes. Try:</p>
      <ul class="examples">{#each EXAMPLES as e}<li><button type="button" onclick={() => run(e)}>{e}</button></li>{/each}</ul>
    {/if}
    {#each entries as entry (entry.id)}
      <div class="entry">
        <div class="q"><span class="prompt">›</span><code>{entry.query}</code><span class="meta">{entry.scope === "workspace" ? "all documents" : ""}</span></div>
        {#if entry.error}<pre class="out error">{entry.error}</pre>
        {:else if !entry.result}<pre class="out muted">…</pre>
        {:else if entry.result.kind === "list"}<ul class="out list">{#each entry.result.items as item}<li>{item}</li>{/each}</ul>
        {:else if entry.result.kind === "table"}
          <div class="out"><table><thead><tr>{#each entry.result.columns as c}<th>{c}</th>{/each}</tr></thead><tbody>{#each entry.result.rows as row}<tr>{#each row as cell}<td>{cell}</td>{/each}</tr>{/each}</tbody></table><span class="meta">{entry.count} row{entry.count === 1 ? "" : "s"}</span></div>
        {:else}<pre class="out">{entry.result.text}</pre>{/if}
      </div>
    {/each}
  </div>
  <div class="prompt-row">
    {#if menu}
      <ul class="typeahead" id="console-typeahead" role="listbox" aria-label="Completions">
        {#each menu.items as item, i}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
          <li role="option" aria-selected={i === selected} class:on={i === selected} onmousedown={e => e.preventDefault()} onclick={() => accept(item)}>
            <span class="kind {item.kind}">{item.kind}</span><span class="label">{item.label}</span>{#if item.detail}<span class="detail">{item.detail}</span>{/if}
          </li>
        {/each}
      </ul>
    {/if}
    <span class="prompt">›</span>
    <input bind:this={field} bind:value={input} type="text" placeholder="Enter a query, e.g. sum(values.value)  ·  Tab to complete" aria-label="Query" autocomplete="off" spellcheck="false"
      role="combobox" aria-autocomplete="list" aria-expanded={!!menu} aria-controls="console-typeahead" onkeydown={keydown} oninput={refresh} onblur={() => setTimeout(() => (menu = null), 100)}>
    <button type="button" class="text" onclick={() => run()} disabled={!input.trim() || busy}>Run</button>
  </div>
</section>
