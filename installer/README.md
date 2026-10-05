# installer

`xmd-installer` installs a verified release binary and optional Zed or VS Code integration. it preserves JSONC comments and existing preferences, repairs missing absolute VS Code server paths, and backs up replaced files

from the repository root:

```sh
make installer
target/release/xmd-installer --editor zed --github-auth
```

use `--editor vscode` for VS Code, or omit `--editor` for only the binary. `--github-auth` uses your GitHub CLI login for private releases; public downloads use curl. `--release VERSION` pins a release

`install.sh` downloads and verifies the native installer, then forwards its arguments. once the site and new installer assets are published, the same setup needs no checkout or build tools:

```sh
curl -fsSL https://xmd.dholtz.com/install.sh | sh -s -- --editor zed
```

repeat the command to update or set up another editor. the bootstrap checks the release and checksum each time, reuses its verified native installer from `${XDG_CACHE_HOME:-~/.cache}/xmd/installer`, and replaces corrupted copies. `XMD_INSTALLER_CACHE_DIR` overrides the cache location; deleting it is safe. release binaries and editor packages are still downloaded on each run

runtime requirements: curl and tar, plus the selected editor; no Python, Node, or Rust. VS Code uses the default profile. `XMD_BIN_DIR`, `XMD_ZED_DATA_DIR`, `XMD_ZED_CONFIG_DIR`, `XMD_CODE_BIN`, `XMD_VSCODE_USER_DATA_DIR`, `XMD_VSCODE_EXTENSIONS_DIR`, and `XMD_BACKUP_DIR` override destinations

checks: `cargo test -p xmd-installer`, then `make installer PROFILE=dev && python3 installer/tests/integration.py`. Python is used only by the fixture tests
