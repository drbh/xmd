//! Signature help, over the one table of built-in calls that also feeds
//! completion, and the prelude's functions, which describe themselves in
//! their `.xmd` source. The attributes and forms modules declare describe
//! themselves, and a note knows them ([`Document::declarations`],
//! [`Document::forms_declared`]).
use crate::{hover::markup, locate::inert};
use lang::document::byte_at;
use lang::eval::Workspace;
use lang::eval::engine::ValueType;
use lang::syntax::Builtin;
use lsp_types::*;
use std::sync::LazyLock;

/// What a call answers with: one value kind wherever the answer has one,
/// and prose for the unions and for the attributes that produce no value at
/// all.
#[derive(Clone, Copy)]
pub enum Outcome {
    Type(ValueType),
    Words(&'static str),
}
impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Type(kind) => kind.as_str(),
            Self::Words(words) => words,
        }
    }
    /// The value type `text` names, or `text` as prose.
    fn read(text: &'static str) -> Self {
        let kinds = <ValueType as strum::VariantArray>::VARIANTS.iter();
        let kind = kinds.copied().find(|kind| kind.as_str() == text);
        kind.map_or(Self::Words(text), Self::Type)
    }
}

// A built-in's tier is declared with it in `syntax`, since the engine itself
// needs to know which built-ins module code alone may call.
use lang::syntax::Tier;

/// One built-in call, as signature help and completion show it:
/// `documentation` explains it and `example` is what signature help fills
/// in. `data/builtins.md` is the only description of them the editor has.
#[derive(Clone)]
pub struct Signature {
    pub name: &'static str,
    pub params: Vec<&'static str>,
    pub result: Outcome,
    pub documentation: &'static str,
    pub example: &'static str,
    pub tier: Tier,
}

/// Every built-in, in `Builtin::ALL` order: what signature help and
/// completion read, from `data/builtins.md` (whose opening paragraphs give
/// its format). Its name and tier come from the `Builtin` itself.
pub static BUILTINS: LazyLock<Vec<Signature>> = LazyLock::new(|| {
    let mut described: Vec<Signature> = (include_str!("../data/builtins.md").split("\n## "))
        .skip(1)
        .map(|section| {
            let mut lines = section.lines().filter(|line| !line.trim().is_empty());
            let mut signature = Signature {
                name: lines.next().unwrap_or_default(),
                params: Vec::new(),
                result: Outcome::Words(""),
                documentation: "",
                example: "",
                tier: Tier::Note,
            };
            for line in lines {
                if let Some(param) = line.strip_prefix("- ") {
                    signature.params.push(param);
                } else if let Some(result) = line.strip_prefix("returns ") {
                    signature.result = Outcome::read(result);
                } else if let Some(example) = line.strip_prefix("example ") {
                    signature.example = example;
                } else {
                    signature.documentation = line;
                }
            }
            signature
        })
        .collect();
    (Builtin::ALL.iter())
        .map(|builtin| {
            let at = described.iter().position(|s| s.name == builtin.as_str());
            let signature = described.swap_remove(at.expect("every built-in is described"));
            Signature {
                name: builtin.as_str(),
                tier: builtin.tier(),
                ..signature
            }
        })
        .collect()
});

pub fn signature(
    ws: &Workspace,
    path: &std::path::Path,
    position: Position,
) -> Option<SignatureHelp> {
    let doc = ws.documents().get(path)?;
    if inert(doc, position) {
        return None;
    }
    let line = doc.line(position.line as usize);
    let byte = byte_at(line, position.character)?;
    let (name, argument) = call_context(&line[..byte])?;
    let (label, params, documentation) = match BUILTINS.iter().find(|f| f.name == name) {
        // A note has no module-tier built-ins, so it is told nothing about them.
        Some(function)
            if function.tier == Tier::Module && !lang::eval::modules::is_module_path(path) =>
        {
            return None;
        }
        Some(function) => (
            format!(
                "{}({}) → {}",
                function.name,
                function.params.join(", "),
                function.result.as_str()
            ),
            function.params.iter().map(|p| p.to_string()).collect(),
            function.documentation.to_string(),
        ),
        // A form a module declares describes itself.
        None if let Some(form) = doc.declared_form(name) => (
            format!("{name}({}) → {}", form.params.join(", "), form.returns),
            form.params.clone(),
            form.documentation.clone(),
        ),
        None if ws.prelude_name(path, name) => {
            let function = ws
                .prelude_functions()
                .into_iter()
                .find(|f| f.name == name)?;
            (
                format!("{}({})", function.name, function.params.join(", ")),
                function.params,
                function.documentation,
            )
        }
        // An attribute a module declares describes itself.
        None => {
            let declared = doc.declared_attribute(name.strip_prefix('@')?)?;
            (
                format!(
                    "{name}({}) → {}",
                    declared.params.join(", "),
                    declared.applies
                ),
                declared.params.clone(),
                declared.documentation.clone(),
            )
        }
    };
    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label,
            documentation: Some(Documentation::MarkupContent(markup(documentation))),
            parameters: Some(
                params
                    .iter()
                    .map(|p| ParameterInformation {
                        label: ParameterLabel::Simple(p.clone()),
                        documentation: None,
                    })
                    .collect(),
            ),
            active_parameter: None,
        }],
        active_signature: Some(0),
        active_parameter: (!params.is_empty())
            .then_some(argument.min(params.len().saturating_sub(1) as u32)),
    })
}

/// Tolerates incomplete calls, quoted strings and nested parentheses.
pub fn call_context(prefix: &str) -> Option<(&str, u32)> {
    let mut stack: Vec<(&str, u32)> = vec![];
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in prefix.char_indices() {
        if quoted {
            if c == '"' && !escaped {
                quoted = false;
            }
            escaped = c == '\\' && !escaped;
            continue;
        }
        match c {
            '"' => quoted = true,
            '(' => {
                let before = prefix[..i].trim_end();
                let name = before
                    .rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '@')
                    .next()
                    .unwrap_or("");
                // After `xs |` the piped value is the first argument.
                let callee = before[..before.len() - name.len()].trim_end();
                let piped = callee.ends_with('|') && !callee.ends_with("||");
                stack.push((name, u32::from(piped)));
            }
            ')' => {
                stack.pop();
            }
            ',' if !(i > 0
                && prefix.as_bytes()[i - 1].is_ascii_digit()
                && prefix.as_bytes().get(i + 1).is_some_and(u8::is_ascii_digit)) =>
            {
                if let Some((_, argument)) = stack.last_mut() {
                    *argument += 1;
                }
            }
            _ => {}
        }
    }
    stack.into_iter().rev().find(|(name, _)| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every built-in is described once, and nothing else is.
    #[test]
    fn every_builtin_is_described_once() {
        let text = include_str!("../data/builtins.md");
        assert_eq!(text.matches("\n## ").count(), Builtin::ALL.len());
        for (builtin, signature) in Builtin::ALL.iter().zip(BUILTINS.iter()) {
            assert_eq!(builtin.as_str(), signature.name);
            assert!(!signature.documentation.is_empty(), "{}", signature.name);
            assert!(!signature.result.as_str().is_empty(), "{}", signature.name);
        }
        let import = &BUILTINS[0];
        let note = concat!(".", lang::common::note_extension!(), "\"");
        assert!(import.name == "import" && import.documentation.contains(note));
    }
}
