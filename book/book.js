// Every block is a note in one shared workspace, served by the same Wasm
// worker as the browser editor. Each block is a contenteditable view that the
// engine paints: semantic tokens for color, inlay hints inline where the IDE
// puts them, diagnostics underlined. No second parser exists here.
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

const esc = s => s.replace(/&/g, "&amp;").replace(/</g, "&lt;");
const legend = await rpc("semanticLegend", {});
const types = legend.tokenTypes;

// The engine's hover is Markdown; keep the few marks it uses and drop the rest.
function renderHover(markdown) {
  return esc(markdown)
    .replace(/```text\n([\s\S]*?)```/g, (_, code) => `<pre>${code.trim()}</pre>`)
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/\[([^\]]+)\]\(&lt;[^)]*&gt;\)/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .split(/\n{2,}/)
    .map(part => part.startsWith("<pre>") ? part : `<p>${part.replace(/\n/g, "<br>")}</p>`)
    .join("");
}

// Text of a view, skipping inlays; <br> and block starts count as newlines.
function textOf(view) {
  let out = "";
  const walk = node => {
    for (const child of node.childNodes) {
      if (child.nodeType === Node.TEXT_NODE) out += child.data;
      else if (child.nodeName === "BR") out += "\n";
      else if (child.classList?.contains("inlay")) continue;
      else {
        if (/^(DIV|P)$/.test(child.nodeName) && out.length && !out.endsWith("\n")) out += "\n";
        walk(child);
      }
    }
  };
  walk(view);
  return out;
}
// Caret as a character offset into textOf(view), and back.
function caretOffset(view) {
  const selection = getSelection();
  if (!selection.rangeCount || !view.contains(selection.anchorNode)) return null;
  const range = selection.getRangeAt(0).cloneRange();
  range.collapse(true);
  const probe = document.createRange();
  probe.setStart(view, 0);
  probe.setEnd(range.startContainer, range.startOffset);
  const fragment = probe.cloneContents();
  const holder = document.createElement("div");
  holder.appendChild(fragment);
  return textOf(holder).length;
}
function setCaret(view, offset) {
  let remaining = offset;
  const walker = document.createTreeWalker(view, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT, {
    acceptNode: node => node.nodeType === Node.ELEMENT_NODE && node.classList.contains("inlay") ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
  });
  let node, last = null;
  while ((node = walker.nextNode())) {
    if (node.nodeType !== Node.TEXT_NODE) { if (node.nodeName === "BR") { if (remaining === 0) break; remaining -= 1; } continue; }
    last = node;
    if (remaining <= node.data.length) {
      const range = document.createRange();
      range.setStart(node, remaining);
      range.collapse(true);
      const selection = getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      return;
    }
    remaining -= node.data.length;
  }
  if (last) {
    const range = document.createRange();
    range.setStart(last, last.data.length);
    range.collapse(true);
    const selection = getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
  }
}
// Text offset under a pointer, or null when it is over an inlay or outside text.
function offsetAt(view, x, y) {
  let node, offset;
  if (document.caretPositionFromPoint) {
    const p = document.caretPositionFromPoint(x, y);
    if (!p) return null;
    node = p.offsetNode; offset = p.offset;
  } else {
    const r = document.caretRangeFromPoint(x, y);
    if (!r) return null;
    node = r.startContainer; offset = r.startOffset;
  }
  if (!view.contains(node) || node.parentElement?.closest(".inlay")) return null;
  const probe = document.createRange();
  probe.setStart(view, 0);
  probe.setEnd(node, offset);
  const holder = document.createElement("div");
  holder.appendChild(probe.cloneContents());
  return textOf(holder).length;
}
const lineChar = (text, offset) => {
  const before = text.slice(0, offset).split("\n");
  return { line: before.length - 1, character: before[before.length - 1].length };
};

function paint(block, snapshot, text) {
  const lines = text.split("\n");
  const spans = lines.map(() => []);
  let row = 0, column = 0;
  const tokens = snapshot.tokens;
  for (let i = 0; i < tokens.length; i += 5) {
    const [dl, ds, len, type, mods] = tokens.slice(i, i + 5);
    if (dl) { row += dl; column = ds; } else column += ds;
    spans[row]?.push({ start: column, end: column + len, cls: `t-${types[type]}${mods & 1 ? " decl" : ""}` });
  }
  const inlays = lines.map(() => new Map());
  for (const hint of snapshot.hints) {
    const at = inlays[hint.position.line];
    if (!at) continue;
    at.set(hint.position.character, [...(at.get(hint.position.character) || []), hint.label]);
  }
  const problems = new Map();
  for (const d of snapshot.diagnostics) {
    for (let l = d.range.start.line; l <= d.range.end.line; l++) {
      const start = l === d.range.start.line ? d.range.start.character : 0;
      const end = l === d.range.end.line ? d.range.end.character : (lines[l] || "").length;
      spans[l]?.push({ start, end, cls: d.severity === 2 ? "warn" : "error" });
    }
    problems.set(`${d.range.start.line}:${d.message}`, d);
  }
  const html = lines.map((line, l) => {
    const cuts = new Set([0, line.length, ...inlays[l].keys()]);
    for (const s of spans[l]) { cuts.add(Math.min(s.start, line.length)); cuts.add(Math.min(s.end, line.length)); }
    const points = [...cuts].filter(p => p <= line.length).sort((a, b) => a - b);
    let out = "";
    const inlayAt = p => { const labels = inlays[l].get(p); return labels ? `<span class="inlay" contenteditable="false">${esc(labels.join(" · "))}</span>` : ""; };
    for (let i = 0; i < points.length; i++) {
      out += inlayAt(points[i]);
      if (i + 1 >= points.length) break;
      const [a, b] = [points[i], points[i + 1]];
      const classes = spans[l].filter(s => s.start <= a && s.end >= b).map(s => s.cls).join(" ");
      out += classes ? `<span class="${classes}">${esc(line.slice(a, b))}</span>` : esc(line.slice(a, b));
    }
    return out;
  }).join("\n");
  const focused = document.activeElement === block.view;
  const caret = focused ? caretOffset(block.view) : null;
  block.view.innerHTML = html;
  if (caret !== null) setCaret(block.view, caret);
  const list = block.querySelector(".problems");
  list.innerHTML = [...problems.values()].map(d => `<li class="${d.severity === 2 ? "warn" : "error"}">Line ${d.range.start.line + 1}: ${esc(d.message)}</li>`).join("");
  list.hidden = problems.size === 0;
  block.querySelector(".status").textContent = snapshot.live ? "live" : "";
}

const blocks = [...document.querySelectorAll(".jot-block")];
let syncing = Promise.resolve();
for (const block of blocks) {
  const view = block.querySelector(".view");
  const hover = block.querySelector(".hover");
  block.view = view;
  block.uri = `file:///workspace/book/${block.dataset.file}`;
  block.version = 0;
  let timer, liveTimer, hoverTimer, hoverKey = "";
  const history = [];
  const status = message => { block.querySelector(".status").textContent = message; };
  const publish = async () => {
    const text = textOf(view), version = ++block.version;
    await (syncing = syncing.then(() => rpc("setDocument", { uri: block.uri, text, version })).catch(() => {}));
    const snapshot = await rpc("analyze", { uri: block.uri });
    if (snapshot.version !== block.version) return;
    paint(block, snapshot, text);
    clearInterval(liveTimer);
    if (snapshot.live) liveTimer = setInterval(async () => {
      const again = await rpc("analyze", { uri: block.uri });
      if (again.version === block.version) paint(block, again, textOf(view));
    }, 1000);
  };
  block.publish = publish;
  block.setText = text => { view.textContent = text; return publish(); };
  const schedule = () => { clearTimeout(timer); timer = setTimeout(() => publish().catch(e => status(e.message)), 120); };
  const remember = () => { const text = textOf(view); if (history.at(-1)?.text !== text) history.push({ text, caret: caretOffset(view) ?? 0 }); if (history.length > 200) history.shift(); };
  view.addEventListener("beforeinput", event => {
    // Keep the view plain text: newlines are "\n", pastes are text only.
    if (event.inputType === "insertParagraph" || event.inputType === "insertLineBreak") {
      event.preventDefault();
      remember();
      document.execCommand("insertText", false, "\n");
    } else if (event.inputType === "insertFromPaste") {
      event.preventDefault();
      remember();
      document.execCommand("insertText", false, event.dataTransfer?.getData("text/plain") || "");
    } else if (event.inputType.startsWith("history")) {
      event.preventDefault();
      if (event.inputType === "historyUndo" && history.length) {
        const previous = history.pop();
        view.textContent = previous.text;
        setCaret(view, previous.caret);
        schedule();
      }
    } else if (event.inputType.startsWith("delete") || event.inputType === "insertText") {
      if (!history.length || history.at(-1).text !== textOf(view)) remember();
    }
  });
  view.addEventListener("input", () => { hideHover(); schedule(); });
  view.addEventListener("keydown", event => {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "z" && !event.shiftKey) {
      event.preventDefault();
      if (history.length) { const previous = history.pop(); view.textContent = previous.text; setCaret(view, previous.caret); schedule(); }
      return;
    }
    if (event.key !== "Enter" && event.key !== "|") return;
    // Format on type through the shared engine: Enter continues checklists, | aligns tables.
    const ch = event.key === "Enter" ? "\n" : "|";
    setTimeout(async () => {
      try {
        const text = textOf(view), version = ++block.version;
        await (syncing = syncing.then(() => rpc("setDocument", { uri: block.uri, text, version })));
        const caret = caretOffset(view) ?? text.length;
        const edits = await rpc("onTypeFormatting", { uri: block.uri, position: lineChar(text, caret), ch });
        if (!edits?.length || textOf(view) !== text) return;
        const lines = text.split("\n");
        const index = ({ line, character }) => lines.slice(0, line).reduce((n, l) => n + l.length + 1, 0) + character;
        let value = text, at = caret;
        for (const edit of [...edits].sort((a, b) => index(b.range.start) - index(a.range.start))) {
          const [a, b] = [index(edit.range.start), index(edit.range.end)];
          value = value.slice(0, a) + edit.newText + value.slice(b);
          if (a <= at) at += edit.newText.length - (Math.min(b, at) - a);
        }
        view.textContent = value;
        setCaret(view, at);
        schedule();
      } catch (e) { status(e.message); }
    }, 0);
  });
  // Hover follows the pointer: the character under it maps straight to the engine.
  const hideHover = () => { hover.hidden = true; hoverKey = ""; clearTimeout(hoverTimer); };
  const placeHover = event => {
    const margin = 12;
    hover.style.maxWidth = `${Math.min(460, window.innerWidth - 2 * margin)}px`;
    let left = event.clientX + 14, top = event.clientY + 18;
    const rect = hover.getBoundingClientRect();
    if (left + rect.width > window.innerWidth - margin) left = Math.max(margin, event.clientX - rect.width - 14);
    if (top + rect.height > window.innerHeight - margin) top = Math.max(margin, event.clientY - rect.height - 18);
    hover.style.left = `${left}px`;
    hover.style.top = `${top}px`;
  };
  const showHover = async event => {
    const text = textOf(view);
    const offset = offsetAt(view, event.clientX, event.clientY);
    if (offset === null) { hideHover(); return; }
    const at = lineChar(text, offset);
    const key = `${at.line}:${at.character}`;
    if (key === hoverKey) { if (!hover.hidden) placeHover(event); return; }
    hoverKey = key;
    try {
      const result = await rpc("hover", { uri: block.uri, position: at });
      if (hoverKey !== key) return;
      if (!result) { hover.hidden = true; return; }
      hover.innerHTML = renderHover(typeof result.contents === "string" ? result.contents : result.contents.value);
      hover.hidden = false;
      placeHover(event);
    } catch { hover.hidden = true; }
  };
  view.addEventListener("mousemove", event => { clearTimeout(hoverTimer); hoverTimer = setTimeout(() => showHover(event), 80); });
  view.addEventListener("mouseleave", hideHover);
  view.addEventListener("wheel", hideHover, { passive: true });
  // Load every note into the shared workspace before analyzing, so cross-note
  // references resolve regardless of order.
  syncing = syncing.then(() => rpc("setDocument", { uri: block.uri, text: textOf(view), version: ++block.version }));
  syncing.catch(e => { $engine.textContent = `Engine failed: ${e.message}`; });
}
try { await syncing; } catch (e) { $engine.textContent = `Engine failed: ${e.message}`; throw e; }
for (const block of blocks) block.publish().catch(e => { block.querySelector(".status").textContent = e.message; });
$engine.textContent = "Rust / WebAssembly · running in this page";
if (new URLSearchParams(location.search).has("test")) window.jotBook = { blocks, rpc, ready: true };
