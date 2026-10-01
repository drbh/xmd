//! Every note in `examples` is something a newcomer opens first, so each
//! one has to work as written: no errors, and no warnings about data that
//! only a refresh could fetch.
use std::{path::Path, process::Command};

#[test]
fn examples_have_no_diagnostics() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = tempfile::tempdir().unwrap();
    let mut names: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(".x.md"))
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no examples in {}", root.display());
    for name in names {
        let output = Command::new(env!("CARGO_BIN_EXE_xmd"))
            .args(["query", &name, "diagnostics | map(.message)", "--json"])
            .arg("--root")
            .arg(&root)
            .env("XDG_CONFIG_HOME", config.path())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.trim() == "[]",
            "{name} reports diagnostics:\n{stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
