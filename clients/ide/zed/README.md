# zed

from a [source checkout](../../../README.md#develop), run `make zed` at the repository root. then choose **Extensions > Install Dev Extension** and select `clients/ide/zed`

for a prebuilt binary and extension, use the [installer](../../../installer) with `--editor zed`. it configures the server path and highlighting, preserving existing preferences

for a dev extension, enable `"semantic_tokens": "full"` under `languages.XMD` in Zed settings. rebuild with `make zed` and reinstall the dev extension after changes
