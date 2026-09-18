import { createWorkspace, defaultUri } from "./workspace.js";
import { lineChar, offsetAt, offsetOfPoint, renderHover, selectionOf, restoreSelection } from "./dom.js";

/** Mount resolved content; controls always execute the engine's versioned actions. */
export async function mount(element, options = {}) {
  const owned = !options.workspace;
  const workspace = options.workspace || createWorkspace(options);
  const uri = options.uri || defaultUri;
  let dead = false, paused = false, latest, hoverTimer, hoverKey = 0;
  const abort = new AbortController();
  const view = element.tagName === "PRE" ? element : element.appendChild(element.ownerDocument.createElement("pre"));
  view.classList.add("wtf");
  view.dataset.layout = options.layout || "source";
  if (options.interactive !== false) view.dataset.interactive = "";
  const controls = options.controls === false ? null : element.ownerDocument.createElement("div");
  if (controls) { controls.className = "wtf-controls"; view.after(controls); }
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
    if (view.innerHTML !== snapshot.html) { view.innerHTML = snapshot.html; restoreSelection(view, selection); }
    if (controls) {
      controls.replaceChildren();
      if (options.interactive !== false) for (const lens of snapshot.lenses) {
        const button = element.ownerDocument.createElement("button");
        button.type = "button";
        button.textContent = lens.command.title;
        button.onclick = () => execute(lens.command, snapshot.versions).catch(error);
        controls.append(button);
      }
    }
    options.onRender?.(snapshot);
  }
  async function execute(command, versions) {
    if (dead) throw new Error("View was destroyed");
    const result = await workspace.execute(command, versions);
    if (!dead && result.open) open(result.open);
    return result;
  }
  const unsubscribe = workspace.subscribe(uri, draw);
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
    refresh: () => workspace.analyze(uri, { force: true }),
    execute,
    pause(value) { paused = value; if (!paused && latest) draw(latest); },
    destroy() {
      if (dead) return;
      dead = true;
      abort.abort(); unsubscribe(); unchange(); clearTimeout(hoverTimer);
      if (hover) hover.hidden = true;
      controls?.remove();
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
