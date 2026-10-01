//! Generates one `#[test]` per snapshot case directory.
//!
//! `tests/snapshots.rs` includes `$OUT_DIR/cases.rs`, so every directory under
//! `tests/cases/` becomes its own test function and cargo's test runner
//! schedules them across cores (and can filter one by name). A case whose
//! `case.json` says `"requires": "browser"` is emitted as an ignored test
//! unless the crate is built with `--features browser`, so it still shows up in
//! plain `cargo test` output instead of vanishing.

use std::{fmt::Write as _, path::Path};

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let cases = Path::new(&manifest).join("tests/cases");
    // Adding or removing a case directory, or a file inside one, re-runs this.
    println!("cargo:rerun-if-changed=tests/cases");
    println!("cargo:rerun-if-changed=build.rs");

    let mut names: Vec<String> = std::fs::read_dir(&cases)
        .expect("tests/cases is missing")
        .map(|entry| entry.expect("tests/cases entry"))
        .filter(|entry| entry.file_type().expect("file type").is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(!names.is_empty(), "No snapshot cases found in tests/cases");

    let mut generated = String::new();
    for name in &names {
        let dir = cases.join(name);
        println!("cargo:rerun-if-changed=tests/cases/{name}");
        let script = std::fs::read_to_string(dir.join("case.json"))
            .unwrap_or_else(|e| panic!("tests/cases/{name}/case.json: {e}"));
        let ident = name.replace(['-', '.', ' '], "_");
        let _ = writeln!(generated, "#[test]");
        if requires_browser(&script) {
            let _ = writeln!(
                generated,
                "#[cfg_attr(not(feature = \"browser\"), ignore = \"requires --features browser\")]"
            );
        }
        let _ = writeln!(generated, "fn {ident}() {{ case({name:?}); }}");
    }

    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("cases.rs");
    std::fs::write(&out, generated).expect("writing cases.rs");
}

/// True when the script's top-level `requires` field is `"browser"`, without
/// pulling a JSON parser into the build script.
fn requires_browser(script: &str) -> bool {
    script
        .split("\"requires\"")
        .nth(1)
        .and_then(|rest| rest.trim_start().strip_prefix(':'))
        .is_some_and(|rest| rest.trim_start().starts_with("\"browser\""))
}
