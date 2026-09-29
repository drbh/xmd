# zed

the zed extension: runs the xmd language server for `.x.md` files,
downloading it if `xmd` is not on your PATH

```bash
mkdir -p xmd-zed && \
  curl -fsSL https://github.com/drbh/xmd/releases/latest/download/xmd-zed.tar.gz | \
  tar -xz -C xmd-zed
```

then Extensions > Install Dev Extension > `xmd-zed`
