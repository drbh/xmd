const assert = require("node:assert/strict");
const vscode = require("vscode");

const command = (name, ...args) => vscode.commands.executeCommand(name, ...args);
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));

async function until(description, check) {
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    const result = await check();
    if (result) return result;
    await delay(100);
  }
  throw new Error(`Timed out: ${description}`);
}
function span(document, text) {
  const index = document.getText().indexOf(text);
  assert(index >= 0, `Missing text: ${text}`);
  return new vscode.Range(document.positionAt(index), document.positionAt(index + text.length));
}
function whole(document) {
  return new vscode.Range(new vscode.Position(0, 0), document.positionAt(document.getText().length));
}
async function hints(document) {
  return await command("vscode.executeInlayHintProvider", document.uri, whole(document)) || [];
}
function label(hint) {
  return typeof hint.label === "string" ? hint.label : hint.label.map(part => part.value).join("");
}
async function replace(document, text, replacement) {
  const edit = new vscode.WorkspaceEdit();
  edit.replace(document.uri, span(document, text), replacement);
  assert(await vscode.workspace.applyEdit(edit));
}
async function runAction(document, text, title) {
  const actions = await command("vscode.executeCodeActionProvider", document.uri, span(document, text));
  const action = actions.find(a => a.title.startsWith(title));
  assert(action, `Missing code action: ${title}`);
  if (action.edit) assert(await vscode.workspace.applyEdit(action.edit));
  const c = typeof action.command === "string" ? action : action.command;
  if (c) await command(c.command, ...(c.arguments || []));
}

exports.run = async function() {
  const root = vscode.workspace.workspaceFolders[0].uri;
  const document = await vscode.workspace.openTextDocument(vscode.Uri.joinPath(root, "main.wtf"));
  const editor = await vscode.window.showTextDocument(document);
  assert.equal(document.languageId, "wtf");
  await vscode.extensions.getExtension("drbh.wtf").activate();

  await until("calculation hints", async () => (await hints(document)).some(h => label(h).includes("$75")));
  const tokens = await command("vscode.provideDocumentSemanticTokens", document.uri);
  assert(tokens?.data.length > 0, "Semantic tokens reach VS Code");
  const symbols = await command("vscode.executeDocumentSymbolProvider", document.uri);
  assert(symbols.length > 0, "Outline symbols reach VS Code");
  const hovers = await command("vscode.executeHoverProvider", document.uri, span(document, "smoke_remaining] :=").start);
  assert(hovers.length > 0, "Calculation hover reaches VS Code");

  const definitions = await command("vscode.executeDefinitionProvider", document.uri, span(document, "smoke_spent\n").start);
  assert(definitions.some(d => (d.targetUri || d.uri).path.endsWith("/values.wtf")), "Cross-file definition");

  await replace(document, "$125", "$150");
  await until("unsaved calculation update", async () => (await hints(document)).some(h => label(h).includes("$100")));
  assert(document.isDirty);

  const originalTask = "- [ ] Try task completion and undo";
  editor.selection = new vscode.Selection(span(document, originalTask).start, span(document, originalTask).start);
  await runAction(document, originalTask, "✓ done");
  await until("task edit", () => document.getText().includes("- [x] Try task completion and undo"));
  await command("undo");
  await until("task undo", () => document.getText().includes(originalTask));

  const lenses = await command("vscode.executeCodeLensProvider", document.uri);
  assert(lenses.some(l => l.command?.title.startsWith("▸ start")), "Timer CodeLens");
  await runAction(document, "[smoke_clock] :=", "▸ start");
  await until("timer edit", () => document.getText().includes("countdown(30s,"));
  const first = (await hints(document)).map(label).join(" ");
  await until("timer value changes without editing", async () => (await hints(document)).map(label).join(" ") !== first);
  await runAction(document, "[smoke_clock] :=", "‖ pause");
  await runAction(document, "[smoke_clock] :=", "↺ reset");

  const rename = await command("vscode.executeDocumentRenameProvider", document.uri, span(document, "smoke_spent\n").start, "smoke_expenses");
  assert(await vscode.workspace.applyEdit(rename));
  const values = await vscode.workspace.openTextDocument(vscode.Uri.joinPath(root, "values.wtf"));
  assert(document.getText().includes("smoke_budget - smoke_expenses"));
  assert(values.getText().includes(":smoke_expenses"));
  assert(values.isDirty, "Cross-file rename preserves unsaved buffers");

  await replace(document, "smoke_budget -", "smoke_budgte -");
  await until("unknown-name diagnostic", () => vscode.languages.getDiagnostics(document.uri).some(d => d.message.includes("smoke_budgte")));
  await replace(document, "smoke_budgte -", "smoke_budget -");
  await until("diagnostic clears", () => !vscode.languages.getDiagnostics(document.uri).some(d => d.message.includes("smoke_budgte")));

  const formatting = await command("vscode.executeFormatDocumentProvider", document.uri, { tabSize: 2, insertSpaces: true });
  assert(formatting.length > 0, "Table formatting");

  await command("wtf.restartServer");
  await until("restart retains unsaved files", async () => (await hints(document)).some(h => label(h).includes("$100")));
  console.log("WTF VS Code smoke test passed: highlighting, hints, hover, navigation, edits/undo, timers, rename, diagnostics, formatting, restart.");
};
