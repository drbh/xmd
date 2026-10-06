#!/usr/bin/env python3
"""Vendor the pinned Markdown block parser with an extension-private export name."""
from pathlib import Path
from urllib.request import urlopen

REVISION = "9a23c1a96c0513d8fc6520972beedd419a973539"
SOURCE = f"https://raw.githubusercontent.com/tree-sitter-grammars/tree-sitter-markdown/{REVISION}/"
ROOT = Path(__file__).resolve().parent
FILES = ["parser.c", "scanner.c", "node-types.json", "tree_sitter/parser.h", "tree_sitter/alloc.h", "tree_sitter/array.h"]


def main():
    # Fetch everything before replacing files; leave the checkout intact on network errors.
    files = {"LICENSE": urlopen(SOURCE + "LICENSE").read()}
    for name in FILES:
        data = urlopen(SOURCE + "tree-sitter-markdown/src/" + name).read()
        files["src/" + name] = data.replace(b"tree_sitter_markdown", b"tree_sitter_xmd")
    for name, data in files.items():
        path = ROOT / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)


if __name__ == "__main__":
    main()
