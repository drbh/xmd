# XMD's Markdown grammar

This is the Markdown block parser from
[tree-sitter-markdown](https://github.com/tree-sitter-grammars/tree-sitter-markdown)
at commit `9a23c1a96c0513d8fc6520972beedd419a973539`, under its [MIT license](LICENSE).

Only the `tree_sitter_markdown` C symbol prefix is changed to `tree_sitter_xmd`.
The syntax is unchanged. This gives the extension its own grammar name, so it
can declare and package its parser without replacing Zed's built-in Markdown
grammar. XMD-specific highlighting and calculations still come from the LSP.

Run `python3 update.py` to reproduce these files from the pinned upstream source.
When updating the parser, change `REVISION` in that script, regenerate, and test
the extension's queries. Commit the grammar before updating the grammar `rev`
in `../extension.toml`, which must point to a published commit containing it.
