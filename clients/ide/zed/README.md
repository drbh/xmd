# zed

from a [source checkout](../../../README.md#develop), run `make zed` at the repository root (Python 3 is also required). then choose **Extensions > Install Dev Extension** and select `clients/ide/zed`

for a prebuilt binary and extension, use the [installer](../../../installer) with `--editor zed`. it configures the server path and highlighting, preserving existing preferences

For a registry or dev installation, add these settings under `languages.XMD`
to enable semantic highlighting, inline calculation results, and task actions:

```json
{
  "languages": {
    "XMD": {
      "semantic_tokens": "full",
      "inlay_hints": { "enabled": true },
      "code_lens": "on"
    }
  }
}
```

Open a file ending in `.x.md` or `.xmd`. The extension uses `xmd` on PATH when
available; otherwise it downloads the language server from the public GitHub
release on first use. Rebuild with `make zed` and reinstall the dev extension
after changes.

The extension packages its own `xmd` grammar, a scoped copy of the upstream
Markdown block parser. It does not replace Zed's Markdown grammar. See
[the grammar source and license](grammar/README.md) for the pinned upstream
revision and regeneration instructions.

`make zed` uses `WASI_SDK_PATH` when set, otherwise reuses Zed's cached WASI SDK
or downloads WASI SDK 25.0 into the extension's ignored `target` directory. The
resulting archive includes the extension, scoped grammar, and their licenses.
