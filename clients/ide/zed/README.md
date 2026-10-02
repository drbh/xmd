# zed

the zed extension: runs the xmd language server for `.x.md` notes and `.xmd` libraries,
downloading it if `xmd` is not on your PATH

```bash
mkdir -p xmd-zed && \
  curl -fsSL https://github.com/drbh/xmd/releases/latest/download/xmd-zed.tar.gz | \
  tar -xz -C xmd-zed
```

then Extensions > Install Dev Extension > `xmd-zed`. keep the folder: zed
loads the extension from it. to update, extract a newer release over it and
reinstall
