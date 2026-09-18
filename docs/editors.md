# Editor setup and smoke test

All native editors launch the same `wtf lsp` executable over stdio. Build it
once with `cargo build --locked`, then configure its absolute path in each
editor. The editor starts and stops the process; no separate server is needed.

| Editor | Setup |
| --- | --- |
| Zed | [Existing dev extension and settings](../README.md#try-it) |
| VS Code | [Local extension](../vscode-extension/README.md) |
| Neovim 0.11+ | [Lua configuration](../neovim/README.md) |
| Helix 25.07.1+ | [TOML configuration and Markdown queries](../helix/README.md) |

Open a notes folder so cross-file lookup has a clear root. For Neovim and Helix,
an empty `.wtf` directory identifies the root of notes that are not in Git.
Initial support uses an explicitly configured binary; there is no automatic
download or marketplace installation.

## Shared smoke test

Copy `examples/editor-smoke` to a scratch folder, create an empty `.wtf`
directory inside it, and open that folder in the editor. Use `main.wtf`:

1. Confirm the remaining value is **$75**. Change `$125` to `$150` and confirm
   it becomes **$100** without saving. Hover the calculation for its explanation.
2. Go to definition on `smoke_spent` in the formula; it should open `values.wtf`.
   Rename it to `smoke_expenses` and check both files, including unsaved buffers.
3. Use a code action to complete the task. Undo it and confirm it is open again.
4. Start the 30-second timer using a code action or CodeLens. Pause, resume, and
   reset it. In editors with inlay refresh, the displayed time should change
   without editing the note. In Helix, retrieve the current value with hover.
5. Mistype a name in the calculation and check the diagnostic and quick fix.
   Correct it and confirm the error clears.
6. Format the document to align its table. Where on-type formatting is enabled,
   press Enter after a checkbox and confirm checklist continuation.
7. Check the outline, completion, and highlighting. Helix uses basic Markdown
   highlighting; other editors receive the Rust semantic tokens.

Automated equivalents run through VS Code's Extension Host and Neovim's real
LSP client. `cargo test --locked --test lsp` also tests the protocol independently
of any editor, including clients without snippet or refresh support.

## Adding another editor

Configure `.wtf` and `wtf lsp`, select a workspace root, enable supported inlay
hints and actions, and map semantic tokens if the editor supports them. Reuse
the smoke notes. Language behavior, command validation, calculations, and source
edits belong in Rust. Editor adapters handle launching, configuration, and
presentation; they do not parse or evaluate WTF.
