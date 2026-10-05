use crate::Editor;
use anyhow::{Context, Result, ensure};
use jsonc_parser::{
    ParseOptions,
    cst::{CstInputValue, CstObject, CstRootNode},
    json,
};
use std::{collections::HashSet, path::Path};

// Reject ambiguous objects rather than editing only one of duplicate settings.
fn unique_keys(object: &CstObject) -> Result<()> {
    let mut names = HashSet::new();
    for prop in object.properties() {
        let name = prop.decoded_name().context("invalid settings key")?;
        ensure!(names.insert(name.clone()), "duplicate settings key: {name}");
        if let Some(child) = prop.value().and_then(|v| v.as_object()) {
            unique_keys(&child)?;
        }
    }
    Ok(())
}

fn add(object: &CstObject, path: &[&str], value: CstInputValue) -> Result<()> {
    let (key, rest) = path.split_first().context("empty settings path")?;
    if rest.is_empty() {
        if object.get(key).is_none() {
            object.append(key, value);
        } else {
            println!("preserved existing setting: {key}");
        }
    } else {
        let child = object
            .object_value_or_create(key)
            .with_context(|| format!("setting {key} must be an object"))?;
        add(&child, rest, value)?;
    }
    Ok(())
}

pub(crate) fn configure(text: &str, editor: Editor, binary: &Path) -> Result<String> {
    let root = CstRootNode::parse(text, &ParseOptions::default())?;
    let object = root
        .object_value_or_create()
        .context("settings must be an object")?;
    unique_keys(&object)?;
    let binary = binary.to_str().context("binary path must be UTF-8")?;
    match editor {
        Editor::Zed => {
            add(
                &object,
                &["languages", "XMD", "semantic_tokens"],
                json!("full"),
            )?;
            add(&object, &["lsp", "xmd", "binary", "path"], json!(binary))?;
            add(
                &object,
                &["lsp", "xmd", "binary", "arguments"],
                json!(["lsp"]),
            )?;
        }
        Editor::Vscode => {
            // A removed checkout must not leave an otherwise successful install
            // pointing at a binary that can no longer start. Keep valid custom
            // paths, PATH commands, and the empty automatic-discovery setting.
            if let Some(property) = object.get("xmd.serverPath")
                && let Some(value) = property.value().and_then(|v| v.as_string_lit())
            {
                let value = value.decoded_value().context("invalid server path")?;
                if Path::new(&value).is_absolute()
                    && std::fs::metadata(&value)
                        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                {
                    println!("replacing missing VS Code server path: {value}");
                    property.set_value(json!(binary));
                }
            }
            add(&object, &["xmd.serverPath"], json!(binary))?;
            add(
                &object,
                &["[xmd]", "editor.semanticHighlighting.enabled"],
                json!(true),
            )?;
        }
    }
    Ok(root.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_comments_preferences_and_repeated_installs() {
        let text = "{\n// keep this\n\"languages\": {\"XMD\": {\"semantic_tokens\": \"combined\",},},\n\"url\": \"https://example.com/*literal*/\",\n}";
        let first = configure(text, Editor::Zed, Path::new("/test/xmd")).unwrap();
        assert!(first.contains("// keep this"));
        assert!(first.contains("\"combined\""));
        assert!(first.contains("https://example.com/*literal*/"));
        assert_eq!(
            configure(&first, Editor::Zed, Path::new("/other/xmd")).unwrap(),
            first
        );
    }
    #[test]
    fn rejects_invalid_or_ambiguous_settings() {
        for text in [
            "{broken}",
            "[]",
            "{\"languages\": false}",
            "{\"a\": 1, \"a\": 2}",
        ] {
            assert!(
                configure(text, Editor::Zed, Path::new("/test/xmd")).is_err(),
                "{text}"
            );
        }
    }
}
