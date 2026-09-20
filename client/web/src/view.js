import { createWorkspace, defaultUri, canonicalUri } from "./workspace.js";
import { lineChar, offsetAt, offsetOfPoint, renderHover, selectionOf, restoreSelection } from "./dom.js";

/** Mount resolved content; controls always execute the engine's versioned actions. */
export async function mount(element, options = {}) {
  const owned = !options.workspace;
  const workspace = options.workspace || createWorkspace(options);
  const uri = canonicalUri(options.uri || defaultUri);
  let dead = false, paused = false, latest, hoverTimer, hoverKey = 0;
  const abort = new AbortController();
  const view = element.tagName === "PRE" ? element : element.appendChild(element.ownerDocument.createElement("pre"));
  view.classList.add("wtf");
  view.dataset.layout = options.layout || "source";
  if (options.interactive !== false) view.dataset.interactive = "";
  // Lenses (complete a task, start a timer) are chips drawn at the end of
  // their line, over the text, so the note itself is never changed. They sit
  // in a layer after the view; the host element becomes the positioning box.
  const controls = options.controls === false ? null : element.ownerDocument.createElement("div");
  if (controls) {
    controls.className = "wtf-controls";
    view.after(controls);
    const host = view.parentElement;
    if (host && getComputedStyle(host).position === "static") host.style.position = "relative";
  }
  let placer = null;
  const error = e => { if (!dead) options.onError?.(e); };
  const open = url => {
    if (options.onOpen) return options.onOpen(url);
    if (/^https?:/.test(url)) globalThis.open(url, "_blank", "noopener,noreferrer");
  };
  function draw(snapshot) {
    if (dead || snapshot.uri !== uri || snapshot.version !== workspace.getDocument(uri)?.version) return;
    latest = snapshot;
    if (paused) return;
    const selection = selectionOf(view);
    // An editable pre cannot place a caret after its final newline. A trailing
    // zero-width, non-editable span gives that position a line box (a <br> would
    // be dropped by the browser as a placeholder). textOf and setCaret skip it.
    const html = options.trailingBreak ? `${snapshot.html}<span class="eol" contenteditable="false">\u200b</span>` : snapshot.html;
    if (view.innerHTML !== html) { view.innerHTML = html; restoreSelection(view, selection); }
    if (controls) {
      controls.replaceChildren();
      if (options.interactive !== false) {
        const byLine = new Map();
        for (const lens of snapshot.lenses) {
          const line = lens.range.start.line;
          if (!byLine.has(line)) { const group = element.ownerDocument.createElement("div"); group.className = "wtf-lenses"; group.dataset.line = line; byLine.set(line, group); controls.append(group); }
          const button = element.ownerDocument.createElement("button");
          button.type = "button";
          button.className = "wtf-lens";
          button.textContent = lens.command.title;
          button.onmousedown = e => e.preventDefault(); // keep the caret where it is
          button.onclick = () => execute(lens.command, snapshot.versions).catch(error);
          byLine.get(line).append(button);
        }
        placeLenses();
      }
    }
    options.onRender?.(snapshot);
  }
  // Chips follow their line's last box, clamped to the host's right edge.
  function placeLenses() {
    if (!controls) return;
    const host = view.parentElement;
    if (!host) return;
    const origin = host.getBoundingClientRect();
    for (const group of controls.children) {
      const line = view.querySelector(`.line[data-line="${group.dataset.line}"]`);
      if (!line) { group.hidden = true; continue; }
      group.hidden = false;
      const rects = line.getClientRects();
      const rect = rects[rects.length - 1] || line.getBoundingClientRect();
      const width = group.offsetWidth || 0;
      group.style.top = `${rect.top - origin.top + host.scrollTop}px`;
      group.style.left = `${Math.max(8, Math.min(rect.right - origin.left + host.scrollLeft + 10, origin.width - width - 8))}px`;
      group.style.height = `${rect.height}px`;
    }
  }
  async function execute(command, versions) {
    if (dead) throw new Error("View was destroyed");
    const result = await workspace.execute(command, versions);
    if (!dead && result.open) open(result.open);
    return result;
  }
  const unsubscribe = workspace.subscribe(uri, draw, { editing: options.editing ?? true });
  if (controls && typeof ResizeObserver !== "undefined") { placer = new ResizeObserver(placeLenses); placer.observe(view); }
  if (controls) element.ownerDocument.fonts?.ready.then(placeLenses);
  const unchange = workspace.onChange(change => { if (!dead && change.uri === uri) options.onChange?.(change); });
  const listen = (type, handler) => view.addEventListener(type, handler, { signal: abort.signal });
  listen("click", event => {
    const link = event.target.closest?.("a[href]");
    if (link && view.contains(link)) { event.preventDefault(); open(link.href); }
  });
  listen("mousedown", event => {
    if (options.interactive === false) return;
    const box = event.target.closest?.(".t-wtfCheckbox, .t-wtfCheckboxChecked");
    if (!box || !view.contains(box)) return;
    event.preventDefault();
    const source = workspace.getDocument(uri)?.source;
    if (source === undefined) return;
    const point = lineChar(source, offsetOfPoint(view, box.firstChild || box, 0));
    workspace.query(uri, "actions", { range: { start: point, end: point } }).then(async result => {
      if (dead || !result) return;
      // Task controls are chosen by the engine; never rewrite checkbox syntax here.
      const action = result.actions.find(a => a.command?.command === "wtf.task");
      if (action) await execute(action.command, result.versions);
    }).catch(error);
  });
  const hover = options.hover;
  if (hover) {
    const hide = () => { clearTimeout(hoverTimer); hoverKey++; hover.hidden = true; };
    listen("mouseleave", hide);
    listen("wheel", hide);
    listen("mousemove", event => {
      hide();
      const key = hoverKey;
      const offset = offsetAt(view, event.clientX, event.clientY);
      if (offset === null) return;
      hoverTimer = setTimeout(async () => {
        try {
          const source = workspace.getDocument(uri)?.source;
          if (source === undefined) return;
          const result = await workspace.query(uri, "hover", { position: lineChar(source, offset) });
          if (dead || key !== hoverKey || !result) return;
          hover.innerHTML = renderHover(typeof result.contents === "string" ? result.contents : result.contents.value);
          hover.hidden = false;
          hover.style.left = `${Math.max(12, Math.min(event.clientX + 14, innerWidth - hover.offsetWidth - 12))}px`;
          hover.style.top = `${Math.max(12, Math.min(event.clientY + 18, innerHeight - hover.offsetHeight - 12))}px`;
        } catch (e) { error(e); }
      }, 80);
    });
  }
  const api = {
    element: view, workspace, uri,
    get snapshot() { return latest; },
    get destroyed() { return dead; },
    getSource: () => workspace.getDocument(uri)?.source,
    setSource: source => dead ? Promise.reject(new Error("View was destroyed")) : workspace.setDocument(uri, source),
    refresh: () => dead ? Promise.reject(new Error("View was destroyed")) : workspace.analyze(uri, { force: true, editing: options.editing ?? true }),
    execute,
    pause(value) { paused = value; if (!paused && latest) draw(latest); },
    destroy() {
      if (dead) return;
      dead = true;
      abort.abort(); unsubscribe(); unchange(); clearTimeout(hoverTimer);
      if (hover) hover.hidden = true;
      controls?.remove(); placer?.disconnect();
      delete view.dataset.interactive;
      if (owned) workspace.destroy();
    },
  };
  try {
    if (options.source !== undefined || !workspace.hasDocument(uri)) await workspace.setDocument(uri, options.source ?? "");
    await api.refresh();
    return api;
  } catch (e) { api.destroy(); throw e; }
}
