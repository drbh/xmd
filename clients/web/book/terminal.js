// A note and a terminal beneath it, the way a person keeps a note file open
// and queries it from a shell. The commands the book ran are already on the
// screen, and a reader can type more. Each runs on the page's engine and
// prints what `xmd` prints: every row as the CLI's text line (the engine hands
// back the same lines), `--json` as the same pretty JSON.

const HELP = (file) => `type a query, or an xmd command as you would in a shell:
  xmd ${file} 'each'
  xmd ${file} 'tasks | filter(fn(t) => !t.done) | map(.title)' --json
  xmd --workspace 'diagnostics'
  xmd render ${file} --format text
a query on its own runs on ${file}. clear empties the screen,
and the up and down arrows go back through what you typed`;

/** A command line's words, as a shell splits them: quotes group, and an
 * unquoted `#` starts a comment, as `#=> $441.33` does in the book. */
export function words(line) {
  const out = [];
  let word = null, quote = null;
  for (const ch of line) {
    if (quote) {
      if (ch === quote) quote = null; else word += ch;
    } else if (ch === "'" || ch === '"') {
      quote = ch; word ??= "";
    } else if (/\s/.test(ch)) {
      if (word !== null) out.push(word), word = null;
    } else if (ch === "#" && word === null) {
      break;
    } else {
      word = (word ?? "") + ch;
    }
  }
  if (quote) throw new Error("unclosed quote");
  if (word !== null) out.push(word);
  return out;
}

/** What `command` prints, run against `workspace`. `file` is the note a bare
 * query runs on. Returns `{ text, error }`, either of which may be empty. */
export async function run(command, { workspace, uriOf, file }) {
  let args;
  try { args = words(command); } catch (e) { return { error: `xmd: ${e.message}` }; }
  if (!args.length) return {};
  if (args[0] !== "xmd") args = ["xmd", file, command.trim()];
  args.shift();
  const flags = new Set(args.filter(a => a.startsWith("--")));
  const rest = args.filter(a => !a.startsWith("--"));
  const note = name => {
    if (!workspace.hasDocument(uriOf(name))) throw new Error(`${name}: No such file or directory`);
    return uriOf(name);
  };
  try {
    if (rest[0] === "render") {
      const format = args[args.indexOf("--format") + 1];
      if (!flags.has("--format") || format !== "text") return { error: "xmd: this terminal renders --format text" };
      const snapshot = await workspace.analyze(note(rest[1]), { force: true });
      const scratch = document.createElement("pre");
      scratch.innerHTML = snapshot.html;
      return { text: scratch.textContent.replace(/\n$/, "") };
    }
    const everywhere = flags.has("--workspace");
    const [target, query] = everywhere ? [null, rest[0]] : rest.length === 1 ? [file, rest[0]] : rest;
    if (!query) return { error: "xmd: name a query" };
    const result = await workspace.request("query", { query, uri: target === null ? null : note(target) });
    // An engine from an older build answers without the CLI's lines.
    if (!Array.isArray(result.lines)) return { error: "xmd: this page's engine is out of date; reload the page" };
    const text = flags.has("--json") ? JSON.stringify(result.rows, null, 2)
      : flags.has("--jsonl") ? result.rows.map(row => JSON.stringify(row)).join("\n")
      : result.lines.join("\n");
    const failed = flags.has("--fail-on-match") && result.rows.length;
    return { text, error: failed ? `xmd: ${result.rows.length} matching result(s)` : "" };
  } catch (e) {
    return { error: `xmd: ${String(e.message ?? e).replace(/^Error:\s*/, "")}` };
  }
}

/** Mount a terminal in `block`, whose code lists the commands to run first. */
export async function mountTerminal(block, { workspace, uriOf, mountEditor, files }) {
  const file = block.dataset.file;
  const commands = block.textContent.split("\n").map(l => l.trim()).filter(l => l && !l.startsWith("#"));
  const doc = block.ownerDocument;
  const el = (tag, className, text) => {
    const node = doc.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  };
  const tab = el("div", "term-tab");
  tab.append(el("span", "term-file", file));
  const reset = el("button", "term-reset", "Reset");
  reset.type = "button";
  reset.title = "Put the note back the way the book wrote it";
  tab.append(reset);
  const host = el("div", "xmd-live");
  const screen = el("div", "term-screen");
  const log = el("div", "term-log");
  log.setAttribute("role", "log");
  log.setAttribute("aria-live", "polite");
  const line = el("form", "term-line");
  const input = el("input");
  Object.assign(input, { type: "text", spellcheck: false, autocomplete: "off", placeholder: `xmd ${file} 'tasks | map(.title)'` });
  input.setAttribute("aria-label", `command to run on ${file}`);
  input.setAttribute("autocapitalize", "off");
  line.append(el("span", "term-prompt", "$"), input);
  screen.append(log, line);
  block.replaceChildren(tab, host, screen);
  const view = await mountEditor(host, { workspace, uri: uriOf(file) });
  reset.addEventListener("click", () => view.setSource(files[file]));

  const print = async (command) => {
    const entry = el("div", "term-entry");
    const cmd = el("div", "term-cmd");
    cmd.append(el("span", "term-prompt", "$"), doc.createTextNode(" " + command));
    entry.append(cmd);
    log.append(entry);
    if (command.trim() === "clear") { log.replaceChildren(); return ""; }
    const { text, error } = command.trim() === "help" ? { text: HELP(file) } : await run(command, { workspace, uriOf, file });
    if (text) entry.append(el("pre", "term-out", text));
    if (error) entry.append(el("pre", "term-err", error));
    log.scrollTop = log.scrollHeight;
    return text ?? "";
  };
  for (const command of commands) await print(command);
  log.scrollTop = 0;

  const history = [];
  let at = -1;
  line.addEventListener("submit", async (event) => {
    event.preventDefault();
    const command = input.value;
    if (!command.trim()) return;
    history.unshift(command);
    at = -1;
    input.value = "";
    await print(command);
  });
  input.addEventListener("keydown", (event) => {
    if (event.key === "ArrowUp" && at < history.length - 1) input.value = history[++at];
    else if (event.key === "ArrowDown" && at > -1) input.value = --at < 0 ? "" : history[at];
    else return;
    event.preventDefault();
  });
  screen.addEventListener("click", () => { if (!getSelection()?.toString()) input.focus(); });
  return { file, view, print, log };
}
