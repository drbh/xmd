//! Enforces the architecture: a crate may only depend on the internal crates
//! listed for it in `[workspace.metadata.layers]` at the repo root; every
//! other crate publishes a curated interface from its root (no public file
//! modules, no glob re-exports, workspace lints on); and the facade `xmd`
//! (this crate's own `src/lib.rs`) only re-exports.
use serde_json::Value;
use std::process::Command;

fn metadata() -> Value {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .expect("run cargo metadata");
    assert!(output.status.success(), "cargo metadata failed");
    serde_json::from_slice(&output.stdout).expect("parse cargo metadata")
}

#[test]
fn crates_only_depend_on_allowed_layers() {
    let metadata = metadata();
    let layers = metadata["metadata"]["layers"]
        .as_object()
        .expect("[workspace.metadata.layers] table is missing from the root Cargo.toml");

    for package in metadata["packages"].as_array().unwrap() {
        let name = package["name"].as_str().unwrap();
        let Some(allowed) = layers.get(name).and_then(Value::as_array) else {
            panic!(
                "crate `{name}` is a workspace member but has no entry in [workspace.metadata.layers]"
            );
        };
        let allowed: Vec<&str> = allowed.iter().map(|v| v.as_str().unwrap()).collect();

        for dep in package["dependencies"].as_array().unwrap() {
            // Only internal, path-based dependencies are governed by the
            // layer table; crates.io dependencies are unrestricted.
            let Some(dep_name) = dep["path"].as_str().and(dep["name"].as_str()) else {
                continue;
            };
            assert!(
                allowed.contains(&dep_name),
                "crate `{name}` depends on `{dep_name}`, which is not in its allowed layers {allowed:?} \
                 (see [workspace.metadata.layers] in the root Cargo.toml)"
            );
        }
    }
}

#[test]
fn facade_only_re_exports() {
    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("read lsp/src/lib.rs");

    // Strip doc/line comments, attributes, and `pub use ...;` items (which
    // may span multiple lines); anything left over is something other than
    // a re-export, which the facade must not contain.
    let without_comments: String = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let without_attributes = strip_spans(&without_comments, "#", "]");
    let without_use = strip_spans(&without_attributes, "pub use", ";");

    assert!(
        without_use.trim().is_empty(),
        "lsp/src/lib.rs (the xmd facade) must contain only doc comments, attributes and `pub use` \
         items; found leftover content: {:?}",
        without_use.trim()
    );
}

#[test]
fn components_publish_a_curated_interface() {
    let metadata = metadata();
    let facade = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

    for package in metadata["packages"].as_array().unwrap() {
        let name = package["name"].as_str().unwrap();
        let manifest = std::path::Path::new(package["manifest_path"].as_str().unwrap());
        let dir = manifest.parent().unwrap();
        // The facade only re-exports, a separate rule (`facade_only_re_exports`).
        if dir == facade {
            continue;
        }
        // `unreachable_pub` from [workspace.lints] flags any `pub` item the
        // crate root doesn't publish.
        let manifest_source = std::fs::read_to_string(manifest).expect("read Cargo.toml");
        assert!(
            manifest_source.contains("[lints]\nworkspace = true"),
            "crate `{name}` must opt into the workspace lints with `[lints] workspace = true`"
        );
        let lib_rs = dir.join("src/lib.rs");
        let Ok(source) = std::fs::read_to_string(&lib_rs) else {
            continue;
        };
        for line in source.lines().map(str::trim) {
            // A file module declared `pub mod x;` leaks its internal layout; an
            // inline namespace `pub mod x { pub use ... }` or `pub(crate) mod x;`
            // is fine.
            if let Some(rest) = line.strip_prefix("pub mod ")
                && let Some(rest) = rest.strip_suffix(';')
            {
                panic!(
                    "{lib_rs:?} declares `pub mod {rest};` — crate `{name}` must keep file \
                     modules private and publish its interface from the root"
                );
            }
            // A glob re-export publishes whatever the module happens to contain.
            assert!(
                !(line.starts_with("pub use") && line.contains("::*")),
                "{lib_rs:?} has a glob re-export `{line}` — crate `{name}` must list \
                 what it publishes"
            );
        }
    }
}

/// Removes every `start ... end` span from `s`, including the delimiters.
fn strip_spans(s: &str, start: &str, end: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find(start) {
        out.push_str(&rest[..i]);
        rest = &rest[i + start.len()..];
        match rest.find(end) {
            Some(j) => rest = &rest[j + end.len()..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}
