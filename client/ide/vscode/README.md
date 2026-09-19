# WTF for VS Code

Language support for `.wtf` notes using the same Rust language server as Zed,
Neovim, and Helix. Requires VS Code 1.91+ and a locally installed WTF binary.

## Install locally

From the repository root, build WTF and package the extension (Node.js 22+):

```sh
cargo build --locked
cd client/ide/vscode
npm ci
npm run package
code --install-extension wtf-0.1.0.vsix
```

Set **WTF: Server Path** in VS Code Settings to the absolute executable path:

```json
{
  "wtf.serverPath": "/absolute/path/to/wtf/target/debug/wtf"
}
```

On Windows, use the absolute path to `target/debug/wtf.exe`. In a remote
workspace, the executable must be installed on that remote host. On macOS,
`/usr/bin/wtf` is an unrelated program; use the binary you built.

Open your notes folder, then a `.wtf` file. Changing the server path restarts
the server. After rebuilding Rust, run **WTF: Restart Language Server**.
Errors and server messages appear in the **WTF** Output channel.

If you previously installed **Jot** (`drbh.jot`), install this **WTF** extension
(`drbh.wtf`) and set `wtf.serverPath`. The older extension handles `.jot` files
and reads `jot.serverPath`; it does not activate for `.wtf` files. Check that the
editor's language mode says **WTF**, then run **Developer: Reload Window** if the
new extension has not attached to an already open file.

The extension requires a trusted workspace because it launches a native
executable. It supports saved file locations, including unsaved edits to those
files; save a new untitled note as `.wtf` to attach the server. A window uses one
server for its workspace folders and the first folder's server-path setting.

## Features

- Calculated-value and timer inlay hints, diagnostics, completion and signatures.
- Hover explanations, definitions/references, rename, outline, and call hierarchy.
- Task and timer controls through CodeLens and code actions, with undoable edits.
- Table formatting and format-on-type for table rows and checklist continuation.
- Semantic highlighting with custom WTF tokens mapped to your current theme.

WTF enables hints, CodeLens, semantic highlighting, two-space indentation, and
format-on-type for its language by default. Your settings can override these.
Colors follow the active theme; the extension does not replace it with Zed's
dark palette. All language features and edits are computed by `wtf lsp`.

## Development and verification

Open this directory in VS Code and press F5 to launch an Extension Development
Host, then configure `wtf.serverPath` there. JavaScript is loaded directly;
there is no compilation or bundling step.

After building the Rust binary, run:

```sh
npm run check
npm test
```

The test runner downloads a separate VS Code instance by default. Set
`VSCODE_EXECUTABLE_PATH` to an installed VS Code executable to use it instead,
and optionally set `WTF_SERVER_PATH` to a different WTF binary. Tests use a
temporary profile and copies of `lang/examples/editor-smoke`, leaving user settings
and notes untouched. On failure, the runner prints the retained logs directory.
