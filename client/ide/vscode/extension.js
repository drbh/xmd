const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");
const { access, chmod, constants, mkdir, readdir, rm, writeFile } = require("node:fs/promises");
const { createHash } = require("node:crypto");
const { execFile } = require("node:child_process");
const { promisify } = require("node:util");
const path = require("node:path");

const REPO = "drbh/jot";
const INSTALL_HINT = "curl -fsSL https://github.com/drbh/jot/releases/latest/download/install.sh | sh";
const CONSENT_KEY = "wtf.downloadConsent";

let client;
let pending = Promise.resolve();
let closing = false;

async function version(binary) {
  const { stdout } = await promisify(execFile)(binary, ["--version"], { timeout: 5000 });
  return /^wtf \d+\.\d+\.\d+/m.test(stdout);
}

// The release asset for this machine, named as the release workflow names it.
function asset() {
  const os = { darwin: "apple-darwin", linux: "unknown-linux-gnu", win32: "pc-windows-msvc" }[process.platform];
  const arch = { x64: "x86_64", arm64: "aarch64" }[process.arch];
  if (!os || !arch || (os === "pc-windows-msvc" && arch !== "x86_64")) {
    throw new Error(`no prebuilt language server for ${process.platform}/${process.arch}. Build one with \`cargo install --git https://github.com/${REPO} wtf\` and set wtf.serverPath.`);
  }
  return `wtf-${arch}-${os}.${os === "pc-windows-msvc" ? "zip" : "tar.gz"}`;
}

async function get(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: ${response.status} ${response.statusText}`);
  return response;
}

// Downloads the server matching this extension's version into the global
// storage directory, so the two can never drift apart, and returns its path.
async function download(context, output) {
  const release = context.extension.packageJSON.version;
  const dir = path.join(context.globalStorageUri.fsPath, "server", release);
  const exe = path.join(dir, process.platform === "win32" ? "wtf.exe" : "wtf");
  try {
    await access(exe, constants.X_OK);
    return exe;
  } catch {}

  const name = asset();
  const base = `https://github.com/${REPO}/releases/download/v${release}`;
  await vscode.window.withProgress({
    location: vscode.ProgressLocation.Notification,
    title: `Downloading the WTF language server ${release}`,
  }, async () => {
    output.appendLine(`downloading ${base}/${name}`);
    const [archive, checksums] = await Promise.all([
      get(`${base}/${name}`).then(r => r.arrayBuffer()),
      get(`${base}/checksums.txt`).then(r => r.text()),
    ]);
    const expected = checksums.split("\n").find(line => line.endsWith(`  ${name}`))?.split(" ")[0];
    if (!expected) throw new Error(`${name} is not listed in the checksums of release v${release}`);
    const actual = createHash("sha256").update(Buffer.from(archive)).digest("hex");
    if (actual !== expected) throw new Error(`checksum mismatch for ${name}: expected ${expected}, got ${actual}`);

    await rm(dir, { recursive: true, force: true });
    await mkdir(dir, { recursive: true });
    const file = path.join(dir, name);
    await writeFile(file, Buffer.from(archive));
    // bsdtar on Windows 10+ and macOS and GNU tar on Linux all read both formats.
    await promisify(execFile)("tar", ["-xf", file, "-C", dir, path.basename(exe)]);
    await rm(file, { force: true });
    if (process.platform !== "win32") await chmod(exe, 0o755);
    output.appendLine(`installed ${exe}`);
  });
  await access(exe, constants.X_OK);
  // Servers of earlier extension versions are not needed once this one works.
  for (const entry of await readdir(path.dirname(dir))) {
    if (entry !== release) await rm(path.join(path.dirname(dir), entry), { recursive: true, force: true });
  }
  return exe;
}

// Where the language server comes from, in order: the wtf.serverPath setting,
// `wtf` on PATH, a server this extension downloaded before, a fresh download
// (asked once; the answer is remembered).
async function resolve(context, output, folder) {
  const config = vscode.workspace.getConfiguration("wtf", folder?.uri);
  const configured = config.get("serverPath", "");
  if (configured) {
    await access(configured, constants.X_OK);
    if (!(await version(configured))) {
      throw new Error(`${configured} is not the WTF notes executable. On macOS, /usr/bin/wtf is an unrelated command; set wtf.serverPath to the installed one.`);
    }
    return configured;
  }
  try {
    if (await version("wtf")) return "wtf";
    output.appendLine("wtf on PATH is not the WTF notes executable (on macOS, /usr/bin/wtf is an unrelated command)");
  } catch {}

  if (!config.get("autoDownload", true)) {
    throw new Error(`wtf is not on PATH. Install it with \`${INSTALL_HINT}\`, or set wtf.serverPath.`);
  }
  if (!context.globalState.get(CONSENT_KEY)) {
    const choice = await vscode.window.showInformationMessage(
      "WTF needs its language server (about 8 MB). Download it from the GitHub release?",
      "Download", "Use PATH instead",
    );
    if (choice !== "Download") {
      throw new Error(`wtf is not on PATH. Install it with \`${INSTALL_HINT}\`, or set wtf.serverPath. Set wtf.autoDownload to false to stop being asked.`);
    }
    await context.globalState.update(CONSENT_KEY, true);
  }
  return download(context, output);
}

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
    const binary = await resolve(context, output, folder);
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
      if (event.affectsConfiguration("wtf")) void restart();
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
