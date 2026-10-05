# vs code

from a [source checkout](../../../README.md#develop), run at the repository root:

```bash
make vscode
code --install-extension dist/xmd.vsix
```

for a prebuilt binary and extension, use the [installer](../../../installer) with `--editor vscode`. it configures the server path and highlighting, preserving existing preferences
