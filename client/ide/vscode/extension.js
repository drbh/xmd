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
  const output = vscode.window.createOutputChannel("WTF", { log: true });
  context.subscriptions.push(output);

  async function start() {
    if (client) {
      await client.dispose();
      client = undefined;
    }
    if (closing) return;

    const folder = vscode.workspace.workspaceFolders?.[0];
    const binary = vscode.workspace.getConfiguration("wtf", folder?.uri).get("serverPath", "");
    if (!path.isAbsolute(binary)) {
      throw new Error("Set wtf.serverPath to the absolute path of your WTF executable (build it with cargo build).");
    }
    await access(binary, constants.X_OK);
    const { stdout } = await promisify(execFile)(binary, ["--version"], { timeout: 5000 });
    if (!/^wtf \d+\.\d+\.\d+/m.test(stdout)) {
      throw new Error("wtf.serverPath is not the WTF notes executable. On macOS, /usr/bin/wtf is an unrelated command.");
    }

    const document = vscode.workspace.textDocuments.find(d => d.languageId === "wtf" && d.uri.scheme === "file");
    const cwd = folder?.uri.fsPath || (document && path.dirname(document.uri.fsPath));
    client = new LanguageClient("wtf", "WTF", {
      command: binary,
      args: ["lsp"],
      options: { cwd },
    }, {
      documentSelector: [{ language: "wtf", scheme: "file" }],
      outputChannel: output,
      markdown: { isTrusted: false },
    });
    await client.start();
  }

  function restart() {
    pending = pending.then(start).catch(error => {
      output.appendLine(String(error));
      // Do not wait for a notification to be dismissed before finishing activation.
      void vscode.window.showErrorMessage(`WTF: ${error.message}`, "Open Settings").then(choice => {
        if (choice) void vscode.commands.executeCommand("workbench.action.openSettings", "wtf.serverPath");
      });
    });
    return pending;
  }

  context.subscriptions.push(
    vscode.commands.registerCommand("wtf.restartServer", restart),
    vscode.workspace.onDidChangeConfiguration(event => {
      if (event.affectsConfiguration("wtf.serverPath")) void restart();
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
