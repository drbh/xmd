# WTF in Neovim

Requires Neovim 0.11+. No plugin manager, nvim-lspconfig, or Mason package is
required: this is a small configuration for Neovim's built-in LSP client.

Build `wtf` with `cargo build --locked` from the repository root, then add this
to `init.lua`, replacing both paths:

```lua
vim.g.wtf_server_path = "/absolute/path/to/wtf/target/debug/wtf"
dofile("/absolute/path/to/wtf/client/ide/neovim/wtf.lua")
```

On Windows, use the `wtf.exe` binary. Use your cargo-built executable; macOS's
`/usr/bin/wtf` is an unrelated program. Open a saved `.wtf` file to attach.
The nearest `.wtf` or `.git` directory identifies the notes root; without
either marker the file's directory is used. Create an empty `.wtf` directory
at your notes root if you need cross-file navigation in a non-Git collection.

The configuration enables inlay hints, completion, semantic highlighting, and
CodeLens. Custom WTF tokens link to your theme's standard highlight groups.
It sets two-space indentation and HTML-style comments for WTF buffers.

| Action | Binding |
| --- | --- |
| Hover | `K` |
| Code actions, including task/timer controls | `<leader>ja` |
| Execute a CodeLens control | `<leader>jl` |
| Format tables | `<leader>jf` |

Other operations use your usual Neovim LSP bindings. Accept built-in completion
with Ctrl-Y. You can edit these buffer-local mappings in `wtf.lua`.

Neovim 0.12+ enables native on-type formatting (checklists and tables) and
server-driven CodeLens refresh. On 0.11, CodeLens refreshes on buffer entry,
cursor idle, and leaving Insert mode. Inlay hints refresh live on both versions.
After rebuilding Rust, restart Neovim or restart its WTF LSP client.

## Verification

From the repository root after `cargo build --locked`:

```sh
nvim --headless -u NONE -i NONE -l client/ide/neovim/smoke.lua
```

This loads the actual configuration with temporary copies of the shared example
notes, checks navigation, calculations, task edits/undo, live timer hints, and
cross-file rename. Set `WTF_SERVER_PATH` to test a different executable.
Run `:checkhealth vim.lsp` to inspect your normal setup.
