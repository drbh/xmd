const { runTests } = require("@vscode/test-electron");
const fs = require("node:fs/promises");
const path = require("node:path");
const os = require("node:os");

async function main() {
  const extension = path.resolve(__dirname, "..");
  const repo = path.dirname(extension);
  const binary = process.env.JOT_SERVER_PATH || path.join(repo, "target", "debug", process.platform === "win32" ? "jot.exe" : "jot");
  await fs.access(binary);
  const temporary = await fs.mkdtemp(path.join(os.tmpdir(), "jot-vscode-"));
  const workspace = path.join(temporary, "notes");
  let passed = false;
  try {
    await fs.cp(path.join(repo, "examples", "editor-smoke"), workspace, { recursive: true });
    await fs.mkdir(path.join(workspace, ".vscode"));
    await fs.writeFile(path.join(workspace, ".vscode", "settings.json"), JSON.stringify({
      "jot.serverPath": binary,
      "files.autoSave": "off",
      "workbench.startupEditor": "none",
      "chat.disableAIFeatures": true,
    }));
    await runTests({
      vscodeExecutablePath: process.env.VSCODE_EXECUTABLE_PATH || undefined,
      extensionDevelopmentPath: extension,
      extensionTestsPath: path.join(__dirname, "suite.js"),
      launchArgs: [
        workspace,
        "--disable-extensions", "--disable-workspace-trust", "--skip-welcome", "--skip-release-notes",
        `--user-data-dir=${path.join(temporary, "user")}`,
        `--extensions-dir=${path.join(temporary, "extensions")}`,
      ],
    });
    passed = true;
  } finally {
    if (passed) await fs.rm(temporary, { recursive: true, force: true });
    else console.error(`Test workspace and logs retained at ${temporary}`);
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
