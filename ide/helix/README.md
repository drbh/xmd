# WTF in Helix

Helix uses the same native `wtf lsp` as the other editors. No editor plugin is
required. This configuration targets Helix 25.07.1+.

1. Build WTF with `cargo build --locked` from the repository root.
2. Merge `ide/helix/languages.toml` into `~/.config/helix/languages.toml`, or into
   `.helix/languages.toml` in your notes project. Replace the placeholder command
   with the absolute path to `target/debug/wtf` (`wtf.exe` on Windows).
3. Copy `ide/helix/runtime/queries/wtf` into
   `~/.config/helix/runtime/queries/wtf`. The two query files inherit Helix's
   bundled Markdown highlighting and inline-language handling; no new grammar
   needs compiling. Adapt the paths if you use a different Helix config directory.
4. Enable inlay hints in your Helix `config.toml`:

```toml
[editor.lsp]
display-inlay-hints = true
```

Open a `.wtf` file and run `hx --health wtf` to check the server and Markdown
parser/query setup. Standard Helix actions provide hover (`Space k`), go to
definition (`gd`), rename (`Space r`), code actions (`Space a`), and formatting
(`:format`). Task completion and timer start/pause/reset appear in code actions.
Use `u` to undo an action and `:lsp-restart` after rebuilding the binary.

The `.wtf` or `.git` root marker scopes cross-file lookup. For a standalone notes
collection, create an empty `.wtf` directory at its root. Use the cargo-built
executable; macOS's `/usr/bin/wtf` is a different program.

## Presentation limits

Helix 25.07.1 uses Markdown syntax highlighting here. It does not consume the
server's richer semantic-token colors or display CodeLens; code actions expose
the same task/timer operations. It also advertises no server-driven inlay-hint
refresh, so running timers are accurate when requested but are not guaranteed to
tick visibly without editing. Hover can retrieve the current value.

Resource-opening actions require `window/showDocument`, which older Helix
versions do not support. Links can still be opened using the editor's usual
navigation facilities. Core calculations, diagnostics, formatting, navigation,
rename, and task/timer edits remain in Rust.

Use `examples/editor-smoke` for an
interactive verification. Basic Markdown colors will differ from WTF's richer
semantic colors in the other editors.
