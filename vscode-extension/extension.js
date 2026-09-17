const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");
const { access, constants } = require("node:fs/promises");
const { execFile } = require("node:child_process");
const { promisify } = require("node:util");
const path = require("node:path");

let client;
let pending = Promise.resolve();
let closing = false;

async function activate(context) {
  const output = vscode.window.createOutputChannel("Jot", { log: true });
  context.subscriptions.push(output);

  async function start() {
    if (client) {
      await client.dispose();
      client = undefined;
    }
    if (closing) return;

    const folder = vscode.workspace.workspaceFolders?.[0];
    const binary = vscode.workspace.getConfiguration("jot", folder?.uri).get("serverPath", "");
    if (!path.isAbsolute(binary)) {
      throw new Error("Set jot.serverPath to the absolute path of your Jot executable (build it with cargo build).");
    }
    await access(binary, constants.X_OK);
    const { stdout } = await promisify(execFile)(binary, ["--version"], { timeout: 5000 });
    if (!/^jot \d+\.\d+\.\d+/m.test(stdout)) {
      throw new Error("jot.serverPath is not the Jot notes executable. On macOS, /usr/bin/jot is an unrelated command.");
    }

    const document = vscode.workspace.textDocuments.find(d => d.languageId === "jot" && d.uri.scheme === "file");
    const cwd = folder?.uri.fsPath || (document && path.dirname(document.uri.fsPath));
    client = new LanguageClient("jot", "Jot", {
      command: binary,
      args: ["lsp"],
      options: { cwd },
    }, {
      documentSelector: [{ language: "jot", scheme: "file" }],
      outputChannel: output,
      markdown: { isTrusted: false },
    });
    await client.start();
  }

  function restart() {
    pending = pending.then(start).catch(error => {
      output.appendLine(String(error));
      // Do not wait for a notification to be dismissed before finishing activation.
      void vscode.window.showErrorMessage(`Jot: ${error.message}`, "Open Settings").then(choice => {
        if (choice) void vscode.commands.executeCommand("workbench.action.openSettings", "jot.serverPath");
      });
    });
    return pending;
  }

  context.subscriptions.push(
    vscode.commands.registerCommand("jot.restartServer", restart),
    vscode.workspace.onDidChangeConfiguration(event => {
      if (event.affectsConfiguration("jot.serverPath")) void restart();
    }),
    // The server reads workspace roots at initialize time.
    vscode.workspace.onDidChangeWorkspaceFolders(() => { void restart(); }),
  );
  await restart();
}

async function deactivate() {
  closing = true;
  await pending;
  if (client) await client.dispose();
  client = undefined;
}

module.exports = { activate, deactivate };
