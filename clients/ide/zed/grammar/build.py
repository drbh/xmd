#!/usr/bin/env python3
"""Build the scoped Markdown parser with the same WASI SDK used by Zed."""
import os
from pathlib import Path
import platform
import subprocess
import tempfile
from urllib.request import urlopen
import shutil

ROOT = Path(__file__).resolve().parent
SDK_VERSION = "25.0"


def sdk_path():
    if os.environ.get("WASI_SDK_PATH"):
        return Path(os.environ["WASI_SDK_PATH"])
    system = {"Darwin": "macos", "Linux": "linux"}.get(platform.system())
    machine = {"arm64": "arm64", "aarch64": "arm64", "x86_64": "x86_64"}.get(platform.machine())
    if not system or not machine:
        raise SystemExit("Set WASI_SDK_PATH to a WASI SDK installation on this platform.")
    zed = (Path.home() / "Library/Application Support/Zed" if system == "macos"
           else Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share")) / "zed")
    installed = zed / "extensions/build/wasi-sdk"
    if (installed / "bin/clang").is_file():
        return installed
    target = ROOT.parent / "target"
    target.mkdir(exist_ok=True)
    name = f"wasi-sdk-{SDK_VERSION}-{machine}-{system}"
    sdk = target / name
    if not (sdk / "bin/clang").is_file():
        url = f"https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-25/{name}.tar.gz"
        print(f"Downloading {url}", flush=True)
        with tempfile.TemporaryDirectory(dir=target) as temp:
            archive = Path(temp) / "sdk.tar.gz"
            with urlopen(url) as response, archive.open("wb") as output:
                shutil.copyfileobj(response, output)
            # Extract the official SDK into a private temporary directory.
            subprocess.run(["tar", "-xzf", str(archive), "-C", temp], check=True)
            (Path(temp) / name).rename(sdk)
    return sdk


def main():
    output = ROOT.parent / "grammars/xmd.wasm"
    output.parent.mkdir(exist_ok=True)
    subprocess.run([
        str(sdk_path() / "bin/clang"), "-fPIC", "-shared", "-Os",
        "-Wl,--export=tree_sitter_xmd", "-o", str(output),
        "-I", str(ROOT / "src"), str(ROOT / "src/parser.c"), str(ROOT / "src/scanner.c"),
    ], check=True)
    print(output)


if __name__ == "__main__":
    main()
