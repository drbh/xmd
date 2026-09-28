//! Enforces the architecture: a crate may only depend on the internal crates
//! listed for it in `[workspace.metadata.layers]` at the repo root; every
//! other crate publishes a curated interface from its root (no public file
//! modules, no glob re-exports, workspace lints on); the facades (`xmd`,
//! `lang`, `runtime`) only re-export, and the crates behind `lang` and
//! `runtime` are private to them; and the portable crates (the
//! language, the services, the renderer and the browser host) do no I/O.
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

/// The facades only re-export: `xmd` (this crate's `src/lib.rs`) and the
/// `lang` and `runtime` facades in front of their private crates. Inline
/// namespaces (`pub mod x { pub use ... }`) group re-exports; nothing else.
#[test]
fn facades_only_re_export() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    for facade in ["cli/src/lib.rs", "lang/src/lib.rs", "runtime/src/lib.rs"] {
        let source = std::fs::read_to_string(root.join(facade)).expect("read a facade");
        // Strip comments, attributes, `pub use ...;` items (which may span
        // lines) and the namespaces around them; anything left over is code.
        let without_comments: String = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let without_attributes = strip_spans(&without_comments, "#", "]");
        let without_use = strip_spans(&without_attributes, "pub use", ";");
        let without_namespaces = strip_spans(&without_use, "pub mod", "{").replace('}', "");
        assert!(
            without_namespaces.trim().is_empty(),
            "{facade} is a facade and must contain only comments, attributes, namespaces and \
             `pub use` items; found leftover content: {:?}",
            without_namespaces.trim()
        );
    }
}

/// `lang/` and `runtime/` each keep their crates private behind one facade:
/// a crate outside the directory may depend on the facade, never on a crate
/// inside it. Derived from the directories, so new crates follow the rule.
#[test]
fn private_crates_stay_behind_their_facade() {
    let metadata = metadata();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let packages = metadata["packages"].as_array().unwrap();
    let dir_of = |package: &Value| {
        std::path::Path::new(package["manifest_path"].as_str().unwrap())
            .parent()
            .unwrap()
            .to_path_buf()
    };
    for area in ["lang", "runtime"] {
        let area = root.join(area);
        let private: Vec<&str> = packages
            .iter()
            .filter(|p| dir_of(p) != area && dir_of(p).starts_with(&area))
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert!(
            !private.is_empty(),
            "expected private crates under {area:?}"
        );
        for package in packages.iter().filter(|p| !dir_of(p).starts_with(&area)) {
            let name = package["name"].as_str().unwrap();
            for dep in package["dependencies"].as_array().unwrap() {
                let dep = dep["name"].as_str().unwrap();
                assert!(
                    !private.contains(&dep),
                    "crate `{name}` depends on `{dep}`, which is private to {area:?}; \
                     depend on its facade instead"
                );
            }
        }
    }
}

#[test]
fn components_publish_a_curated_interface() {
    let metadata = metadata();
    let facade = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

    for package in metadata["packages"].as_array().unwrap() {
        let name = package["name"].as_str().unwrap();
        let manifest = std::path::Path::new(package["manifest_path"].as_str().unwrap());
        let dir = manifest.parent().unwrap();
        // The xmd facade only re-exports and opts out of the lints; a separate
        // rule (`facades_only_re_export`) covers it.
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

/// The portable crates run identically on every host, the browser included:
/// `lang/core` (the language itself), `runtime/services` (the language
/// services), `runtime/renderer` and `browser`. Reading files, running
/// programs and talking to the network belong to `runtime/host`, which the
/// native hosts pass in where the services need files (`NoteFiles`).
#[test]
fn portable_crates_do_no_io() {
    const FORBIDDEN_CRATES: [&str; 5] = ["tokio", "ignore", "feed-rs", "reqwest", "notify"];
    const FORBIDDEN_CODE: [&str; 5] = [
        "std::fs",
        "std::process",
        "std::net",
        "tokio::",
        "Command::new",
    ];
    let metadata = metadata();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let portable = [
        root.join("lang"),
        root.join("runtime/services"),
        root.join("runtime/renderer"),
        root.join("browser"),
    ];
    let mut checked = 0;
    for package in metadata["packages"].as_array().unwrap() {
        let name = package["name"].as_str().unwrap();
        let manifest = std::path::Path::new(package["manifest_path"].as_str().unwrap());
        let dir = manifest.parent().unwrap();
        if !portable.iter().any(|p| dir.starts_with(p)) {
            continue;
        }
        checked += 1;
        for dep in package["dependencies"].as_array().unwrap() {
            let dep = dep["name"].as_str().unwrap();
            assert!(
                !FORBIDDEN_CRATES.contains(&dep),
                "portable crate `{name}` depends on `{dep}`; I/O belongs to runtime/host"
            );
        }
        let mut pending = vec![dir.join("src")];
        while let Some(path) = pending.pop() {
            for entry in std::fs::read_dir(&path).expect("read source directory") {
                let path = entry.expect("directory entry").path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let source = std::fs::read_to_string(&path).expect("read source");
                    for pattern in FORBIDDEN_CODE {
                        assert!(
                            !source.contains(pattern),
                            "{path:?} uses `{pattern}`; portable crates stay free of I/O, \
                             which belongs to runtime/host"
                        );
                    }
                }
            }
        }
    }
    assert!(
        checked >= 7,
        "expected the portable crates, found {checked}"
    );
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
