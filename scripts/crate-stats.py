#!/usr/bin/env python3
"""Lines of Rust and public API size for every crate in the repository.

    scripts/crate-stats.py            # lines and public API
    scripts/crate-stats.py --no-api   # lines only; no nightly toolchain needed
    scripts/crate-stats.py --json     # machine-readable

Lines cover every .rs file in a crate's directory (src, tests, build.rs, ...),
minus target/ and nested crates. "code" excludes blank and comment lines.

The public API comes from rustdoc's JSON output, which needs a nightly
toolchain (--toolchain, default "nightly"). It walks the crate from its root
the way a dependent sees it: `pub` items inside private modules do not count,
re-exports do, and each item counts once however many paths reach it.
Methods are the public inherent methods of reachable types; trait impls,
fields and variants are not counted. Modules are namespaces, so the total
leaves them out.
"""

import argparse
import json
import subprocess
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "target" / "crate-stats"
KINDS = {
    "module": "mods",
    "struct": "structs",
    "enum": "enums",
    "union": "structs",
    "trait": "traits",
    "trait_alias": "traits",
    "function": "fns",
    "constant": "consts",
    "static": "consts",
    "type_alias": "types",
    "macro": "macros",
    "proc_macro": "macros",
}
API_COLUMNS = ["mods", "structs", "enums", "traits", "fns", "methods", "consts", "types", "macros"]


def crates():
    """Every Cargo package tracked in git, including ones outside the workspace."""
    manifests = subprocess.run(
        ["git", "ls-files", "*Cargo.toml", "Cargo.toml"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout.split()
    found = []
    for manifest in manifests:
        meta = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1",
             "--manifest-path", str(ROOT / manifest)],
            cwd=ROOT, capture_output=True, text=True,
        )
        if meta.returncode:
            print(f"warning: skipping {manifest}: {meta.stderr.strip()}", file=sys.stderr)
            continue
        for package in json.loads(meta.stdout)["packages"]:
            path = Path(package["manifest_path"])
            if path == (ROOT / manifest).resolve():
                lib = next((t for t in package["targets"] if "lib" in t["kind"]
                            or "rlib" in t["kind"] or "cdylib" in t["kind"]), None)
                found.append({"name": package["name"], "manifest": path, "lib": lib})
    return found


def count_lines(files):
    totals = Counter()
    for file in files:
        in_block = False
        for line in file.read_text(errors="replace").splitlines():
            text = line.strip()
            totals["lines"] += 1
            if in_block:
                totals["comments"] += 1
                in_block = "*/" not in text
            elif not text:
                totals["blank"] += 1
            elif text.startswith("//"):
                totals["comments"] += 1
            elif text.startswith("/*"):
                totals["comments"] += 1
                in_block = "*/" not in text
            else:
                totals["code"] += 1
        totals["files"] += 1
    return totals


def rust_files(crate_dir, other_crate_dirs):
    for file in sorted(crate_dir.rglob("*.rs")):
        parts = file.relative_to(crate_dir).parts
        if "target" in parts or "node_modules" in parts:
            continue
        # Skip crates nested inside this one, such as a crate nested in another's directory.
        if any(crate_dir in d.parents and d in file.parents for d in other_crate_dirs):
            continue
        yield file


def rustdoc_json(crate, toolchain):
    target = crate["lib"]["name"].replace("-", "_")
    result = subprocess.run(
        ["cargo", f"+{toolchain}", "rustdoc", "-q", "--lib",
         "--manifest-path", str(crate["manifest"]), "--target-dir", str(TARGET),
         "--", "-Z", "unstable-options", "--output-format", "json"],
        cwd=ROOT, capture_output=True, text=True,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.strip().splitlines()[-1] if result.stderr else "rustdoc failed")
    return json.loads((TARGET / "doc" / f"{target}.json").read_text())


def public_api(doc):
    index = doc["index"]
    item = lambda i: index.get(str(i))
    seen, counts = set(), Counter()

    def visit(item_id):
        it = item(item_id)
        if it is None or it["crate_id"] != 0 or item_id in seen:
            return
        seen.add(item_id)
        kind, inner = next(iter(it["inner"].items()))
        if kind == "use":
            target = inner.get("id")
            if inner.get("is_glob"):
                glob = item(target) if target is not None else None
                if glob and "module" in glob["inner"]:
                    for child in glob["inner"]["module"]["items"]:
                        if item(child) and item(child)["visibility"] == "public":
                            visit(child)
            elif target is not None:
                visit(target)
            return
        if kind in KINDS:
            counts[KINDS[kind]] += 1
        if kind == "module":
            for child in inner["items"]:
                child_item = item(child)
                if child_item and child_item["visibility"] == "public":
                    visit(child)
        for impl_id in inner.get("impls", []) if isinstance(inner, dict) else []:
            impl = (item(impl_id) or {}).get("inner", {}).get("impl")
            if impl and impl["trait"] is None and not impl.get("is_synthetic"):
                counts["methods"] += sum(
                    1 for m in impl["items"]
                    if item(m) and item(m)["visibility"] == "public" and "function" in item(m)["inner"]
                )

    visit(doc["root"])
    counts["total"] = sum(n for k, n in counts.items() if k != "mods")
    return counts


def table(rows, columns):
    widths = [max(len(c), *(len(str(r.get(c, ""))) for r in rows)) for c in columns]
    line = lambda values: "  ".join(
        str(v).ljust(w) if i == 0 else str(v).rjust(w) for i, (v, w) in enumerate(zip(values, widths))
    )
    print(line(columns))
    print(line("-" * w for w in widths))
    for row in rows:
        print(line(row.get(c, 0) for c in columns))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--no-api", action="store_true", help="skip the public API count")
    parser.add_argument("--toolchain", default="nightly", help="toolchain for rustdoc JSON")
    parser.add_argument("--json", action="store_true", help="print JSON instead of tables")
    args = parser.parse_args()

    found = crates()
    dirs = [c["manifest"].parent for c in found]
    rows = []
    for crate in found:
        row = {"crate": crate["name"], "path": str(crate["manifest"].parent.relative_to(ROOT))}
        row.update(count_lines(rust_files(crate["manifest"].parent, dirs)))
        if not args.no_api:
            if crate["lib"] is None:
                row["total"] = "no library"
            else:
                try:
                    row.update(public_api(rustdoc_json(crate, args.toolchain)))
                except (RuntimeError, OSError, json.JSONDecodeError) as error:
                    row["total"] = "error"
                    print(f"warning: {crate['name']}: {error}", file=sys.stderr)
        rows.append(row)

    if args.json:
        print(json.dumps(rows, indent=2))
        return
    table(rows, ["crate", "path", "files", "lines", "code", "comments", "blank"])
    if not args.no_api:
        print()
        table(rows, ["crate", *API_COLUMNS, "total"])


if __name__ == "__main__":
    main()
