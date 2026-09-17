// Every block is a note in one shared workspace, served by the same Wasm
// worker as the browser editor. A small overlay editor renders the engine's
// semantic tokens, inlay hints, and diagnostics; no second parser exists here.
const $engine = document.getElementById("engine");
const worker = new Worker(new URL("../worker.js", import.meta.url), { type: "module" });
const pending = new Map();
let nextId = 0;
function rpc(method, params) {
  return new Promise((resolve, reject) => {
    const id = ++nextId;
    pending.set(id, { resolve, reject });
    worker.postMessage({ id, method, params });
  });
}
worker.onmessage = ({ data }) => {
  const request = pending.get(data.id);
  if (!request) return;
  pending.delete(data.id);
  if (data.ok) request.resolve(data.result); else request.reject(new Error(data.error));
};
worker.onerror = event => { $engine.textContent = `Engine failed: ${event.message || "run bash web/build.sh"}`; };

const blocks = [...document.querySelectorAll(".jot-block")];
const palette = ["comment", "keyword", "number", "variable", "operator", "string", "heading", "function", "property", "decorator"];
const legend = await rpc("semanticLegend", {});
const types = legend.tokenTypes;

function offsets(line) {
  // UTF-16 column → JS index is identity; tokens arrive in UTF-16 units.
  return line;
}
function render(block, snapshot, text) {
  const lines = text.split("\n");
  const spans = lines.map(() => []);
  let row = 0, column = 0;
  const tokens = snapshot.tokens;
  for (let i = 0; i < tokens.length; i += 5) {
    const [dl, ds, len, type, mods] = tokens.slice(i, i + 5);
    if (dl) { row += dl; column = ds; } else column += ds;
    spans[row]?.push({ start: column, end: column + len, cls: `t-${types[type]}${mods & 1 ? " decl" : ""}` });
  }
  const hints = new Map();
  for (const hint of snapshot.hints) {
    const key = hint.position.line;
    hints.set(key, [...(hints.get(key) || []), hint.label]);
  }
  const problems = new Map();
  for (const d of snapshot.diagnostics) {
    for (let l = d.range.start.line; l <= d.range.end.line; l++) {
      const start = l === d.range.start.line ? d.range.start.character : 0;
      const end = l === d.range.end.line ? d.range.end.character : (lines[l] || "").length;
      spans[l]?.push({ start, end, cls: d.severity === 2 ? "warn" : "error", top: true });
    }
    problems.set(`${d.range.start.line}:${d.message}`, d);
  }
  const esc = s => s.replace(/&/g, "&amp;").replace(/</g, "&lt;");
  block.view.innerHTML = lines.map((line, l) => {
    const cuts = new Set([0, line.length]);
    for (const s of spans[l]) { cuts.add(Math.min(s.start, line.length)); cuts.add(Math.min(s.end, line.length)); }
    const points = [...cuts].sort((a, b) => a - b);
    let html = "";
    for (let i = 0; i + 1 < points.length; i++) {
      const [a, b] = [points[i], points[i + 1]];
      const classes = spans[l].filter(s => s.start <= a && s.end >= b).map(s => s.cls).join(" ");
      html += classes ? `<span class="${classes}">${esc(line.slice(a, b))}</span>` : esc(line.slice(a, b));
    }
    const inlay = hints.get(l);
    if (inlay) html += `<span class="inlay">${esc(inlay.join(" · "))}</span>`;
    return html || " ";
  }).join("\n");
  const list = block.querySelector(".problems");
  list.innerHTML = [...problems.values()].map(d => `<li class="${d.severity === 2 ? "warn" : "error"}">Line ${d.range.start.line + 1}: ${esc(d.message)}</li>`).join("");
  list.hidden = problems.size === 0;
  block.querySelector(".status").textContent = snapshot.live ? "live" : "";
}

let syncing = Promise.resolve();
for (const [index, block] of blocks.entries()) {
  const textarea = block.querySelector("textarea");
  const view = block.querySelector(".view");
  block.view = view;
  block.uri = `file:///workspace/book/${block.dataset.file}`;
  block.version = 0;
  const hover = block.querySelector(".hover");
  let timer, liveTimer;
  const publish = async () => {
    const text = textarea.value, version = ++block.version;
    await (syncing = syncing.then(() => rpc("setDocument", { uri: block.uri, text, version })).catch(() => {}));
    const snapshot = await rpc("analyze", { uri: block.uri });
    if (snapshot.version !== block.version) return;
    render(block, snapshot, text);
    clearInterval(liveTimer);
    if (snapshot.live) liveTimer = setInterval(async () => {
      const again = await rpc("analyze", { uri: block.uri });
      if (again.version === block.version) render(block, again, textarea.value);
    }, 1000);
  };
  block.publish = publish;
  const schedule = () => { clearTimeout(timer); timer = setTimeout(() => publish().catch(showError), 120); };
  const showError = e => { block.querySelector(".status").textContent = e.message; };
  textarea.addEventListener("input", schedule);
  const position = () => {
    const before = textarea.value.slice(0, textarea.selectionStart).split("\n");
    return { line: before.length - 1, character: before[before.length - 1].length };
  };
  textarea.addEventListener("keydown", async event => {
    // Format on type through the shared engine: Enter continues checklists, | aligns tables.
    if (event.key !== "Enter" && event.key !== "|") return;
    const ch = event.key === "Enter" ? "\n" : "|";
    const start = textarea.selectionStart;
    setTimeout(async () => {
      try {
        const text = textarea.value, version = ++block.version;
        await (syncing = syncing.then(() => rpc("setDocument", { uri: block.uri, text, version })));
        const edits = await rpc("onTypeFormatting", { uri: block.uri, position: position(), ch });
        if (!edits?.length || textarea.value !== text) return;
        const lines = text.split("\n");
        const index = ({ line, character }) => lines.slice(0, line).reduce((n, l) => n + l.length + 1, 0) + character;
        let value = text, caret = textarea.selectionStart;
        for (const edit of [...edits].sort((a, b) => index(b.range.start) - index(a.range.start))) {
          const [a, b] = [index(edit.range.start), index(edit.range.end)];
          value = value.slice(0, a) + edit.newText + value.slice(b);
          if (a <= caret) caret += edit.newText.length - (Math.min(b, caret) - a);
        }
        textarea.value = value;
        textarea.setSelectionRange(caret, caret);
        schedule();
      } catch (e) { showError(e); }
    }, 0);
    void start;
  });
  const showHover = async () => {
    try {
      const result = await rpc("hover", { uri: block.uri, position: position() });
      if (!result) { hover.hidden = true; return; }
      const value = typeof result.contents === "string" ? result.contents : result.contents.value;
      hover.textContent = value.replace(/\*\*/g, "").replace(/\]\(<[^>]*>\)/g, "]").replace(/```text\n?|```/g, "");
      hover.hidden = false;
    } catch { hover.hidden = true; }
  };
  textarea.addEventListener("click", showHover);
  textarea.addEventListener("keyup", event => { if (event.key.startsWith("Arrow")) showHover(); });
  textarea.addEventListener("blur", () => { hover.hidden = true; });
  // Load every note into the shared workspace before analyzing, so cross-note
  // references resolve regardless of order.
  syncing = syncing.then(() => rpc("setDocument", { uri: block.uri, text: textarea.value, version: ++block.version }));
  syncing.catch(e => { $engine.textContent = `Engine failed: ${e.message}`; });
  void index;
}
try {
  await syncing;
} catch (e) {
  $engine.textContent = `Engine failed: ${e.message}`;
  throw e;
}
for (const block of blocks) block.publish().catch(e => { block.querySelector(".status").textContent = e.message; });
$engine.textContent = "Rust / WebAssembly · running in this page";
if (new URLSearchParams(location.search).has("test")) window.jotBook = { blocks, rpc, ready: true };
void palette; void offsets;
