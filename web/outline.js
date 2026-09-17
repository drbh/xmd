// Client UI only. Hierarchy, values, and ranges arrive as LSP DocumentSymbol[];
// there is deliberately no Jot parser or evaluator in this module.
export function createOutline(editor, elements) {
  const { list, filter, empty } = elements;
  const collapsed = new Map();
  let model, symbols = [], version = 0, fingerprint = "", rows = [];
  function reset(nextModel) {
    if (model !== nextModel) filter.value = "";
    model = nextModel; symbols = []; rows = []; fingerprint = "";
    list.replaceChildren(); empty.hidden = false; empty.textContent = "Loading symbols…";
  }
  function update(nextModel, snapshot) {
    if (nextModel !== editor.getModel() || snapshot.version !== nextModel.getVersionId()) return;
    if (model !== nextModel) reset(nextModel);
    version = snapshot.version;
    symbols = snapshot.symbols || [];
    const next = JSON.stringify(symbols);
    if (next !== fingerprint) { fingerprint = next; render(); }
  }
  function highlight() {
    const p = editor.getPosition();
    let current;
    if (p) {
      const position = { line: p.lineNumber - 1, character: p.column - 1 };
      const before = (a, b) => a.line < b.line || a.line === b.line && a.character <= b.character;
      for (const row of rows) {
        if (before(row.symbol.range.start, position) && before(position, row.symbol.range.end)) current = row.button;
      }
    }
    for (const { button } of rows) {
      if (button === current) button.setAttribute("aria-current", "location");
      else button.removeAttribute("aria-current");
    }
  }
  function render() {
    const focused = list.contains(document.activeElement) ? document.activeElement.dataset.key : null;
    list.replaceChildren(); rows = [];
    const query = filter.value.trim().toLocaleLowerCase();
    const matches = s => `${s.name} ${s.detail || ""}`.toLocaleLowerCase().includes(query);
    const branchMatches = s => matches(s) || s.children?.some(branchMatches);
    const uri = model.uri.toString();
    if (!collapsed.has(uri)) collapsed.set(uri, new Set());
    const closed = collapsed.get(uri);
    function append(items, depth = 0, parent = "", parentMatch = false) {
      items.forEach((symbol, index) => {
        if (query && !parentMatch && !branchMatches(symbol)) return;
        const key = `${parent}/${index}:${symbol.name}`;
        const row = document.createElement("div"); row.className = "outline-row"; row.style.setProperty("--depth", depth);
        if (symbol.children?.length) {
          const toggle = document.createElement("button"); toggle.className = "outline-toggle";
          const expanded = !!query || !closed.has(key);
          toggle.textContent = expanded ? "▾" : "▸";
          toggle.setAttribute("aria-label", `${expanded ? "Collapse" : "Expand"} ${symbol.name}`);
          toggle.setAttribute("aria-expanded", String(expanded)); toggle.dataset.key = `toggle:${key}`;
          toggle.disabled = !!query;
          toggle.onclick = () => { if (closed.has(key)) closed.delete(key); else closed.add(key); render(); };
          row.append(toggle);
        } else {
          const spacer = document.createElement("span"); spacer.className = "outline-spacer"; row.append(spacer);
        }
        const button = document.createElement("button"); button.className = "outline-symbol";
        button.dataset.key = key; button.setAttribute("aria-label", symbol.name);
        button.title = `${symbol.name}${symbol.detail ? ` — ${symbol.detail}` : ""}`;
        const name = document.createElement("span"); name.className = "outline-name"; name.textContent = symbol.name; button.append(name);
        if (symbol.detail) {
          const detail = document.createElement("span"); detail.className = "outline-detail"; detail.textContent = symbol.detail; button.append(detail);
        }
        button.onclick = () => {
          // A source edit invalidates these ranges until the next LSP snapshot.
          if (editor.getModel() !== model || model.getVersionId() !== version) return;
          const r = symbol.selectionRange;
          const selection = { startLineNumber: r.start.line + 1, startColumn: r.start.character + 1, endLineNumber: r.end.line + 1, endColumn: r.end.character + 1 };
          editor.setSelection(selection); editor.revealRangeInCenter(selection); editor.focus();
        };
        row.append(button); list.append(row); rows.push({ button, symbol });
        if (symbol.children && (query || !closed.has(key))) append(symbol.children, depth + 1, key, parentMatch || matches(symbol));
      });
    }
    append(symbols);
    empty.hidden = rows.length > 0;
    empty.textContent = query ? "No matching symbols." : "Add a heading, task, or named value to see it here.";
    highlight();
    if (focused) [...list.querySelectorAll("button")].find(b => b.dataset.key === focused)?.focus();
  }
  filter.addEventListener("input", render);
  editor.onDidChangeCursorPosition(highlight);
  editor.onDidChangeModelContent(() => reset(editor.getModel()));
  return { reset, update };
}
