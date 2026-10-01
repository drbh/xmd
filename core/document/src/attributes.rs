//! What a line's `@key(value)` attributes mean: the language owns none, and
//! feature modules declare the ones they own ([`Declaration`]), which a note
//! is parsed with. From that, how each value is painted (a date, a time
//! stamp, an expression or text) and the problems of a repeated, unknown or
//! unclosed attribute. Every line that writes attributes is kept as it was
//! read ([`Attributed`]), so what a declared attribute means is read off it
//! later, by the module that owns it.
use crate::blocks::Line;
use crate::declared::On;
use crate::document::Document;
use crate::inline::Attribute;
use crate::tree::{HighlightKind, Tree};
use std::collections::BTreeMap;
use std::sync::Arc;
use syntax::AttributeValue;

/// An attribute a feature module declares in its manifest's `attributes`:
/// the key a note writes after `@`, what its value holds, and how signature
/// help, completion and the reference describe it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub key: String,
    /// The id of the module that declares it.
    pub module: String,
    pub value: AttributeValue,
    /// The kinds of tagged record a [`AttributeValue::Tagged`] value takes,
    /// as the module named them; empty for every other value.
    pub kinds: Vec<String>,
    pub params: Vec<String>,
    /// Which lines take it, as signature help words it.
    pub applies: String,
    pub documentation: String,
    pub example: String,
    /// The values completion offers inside it.
    pub values: Vec<String>,
    /// Whether it is live only on a checklist item (a list item with a
    /// checkbox): written on any other line, it stays prose.
    pub checkbox: bool,
}
impl Declaration {
    /// Whether it is live on a line that is, or is not, a checklist item.
    pub(crate) fn live_on(&self, checkbox: bool) -> bool {
        checkbox || !self.checkbox
    }
}

/// A line that writes attributes: a list item, a table row or a line of
/// prose, whatever else it is.
#[derive(Clone, Debug)]
pub struct Attributed {
    pub line: usize,
    pub on: On,
    /// Whether it is a task: a list item with a checkbox.
    pub checkbox: bool,
    /// Its text from where it starts (past a list marker and any checkbox)
    /// up to its first attribute, trimmed.
    pub title: String,
    /// Its attributes by key, the last of a repeated key winning.
    pub attributes: BTreeMap<String, Attribute>,
}

impl Document {
    /// What an attribute's value holds, as the module that declares it
    /// says. None for an unknown key.
    pub fn attribute_value(&self, key: &str) -> Option<AttributeValue> {
        self.declared_attribute(key).map(|d| d.value)
    }
    /// The declaration of an attribute a module owns.
    pub fn declared_attribute(&self, key: &str) -> Option<&Declaration> {
        self.declarations
            .iter()
            .find(|d| d.key == key)
            .map(Arc::as_ref)
    }
    /// The declared attributes the note was parsed with.
    pub fn declarations(&self) -> &[Arc<Declaration>] {
        &self.declarations
    }
    /// The lines whose attributes are live: every checklist item, and every
    /// other line that writes an attribute a module declares live there.
    /// Their attributes are painted, checked and evaluated; elsewhere
    /// `@key(value)` is prose.
    pub fn claimed(&self) -> impl Iterator<Item = &Attributed> {
        self.attributed.iter().filter(|a| {
            a.checkbox
                || a.attributes
                    .keys()
                    .any(|k| self.declared_attribute(k).is_some_and(|d| d.live_on(false)))
        })
    }
    /// The declared attributes a claimed line's value is read for: every
    /// one it writes that is live on it.
    pub fn live_attributes<'a>(
        &'a self,
        line: &'a Attributed,
    ) -> impl Iterator<Item = (&'a Declaration, &'a Attribute)> {
        line.attributes.iter().filter_map(|(key, attribute)| {
            let declared = self.declared_attribute(key)?;
            declared
                .live_on(line.checkbox)
                .then_some((declared, attribute))
        })
    }
    /// Every attribute of the lines [`claimed`](Self::claimed), by key.
    pub fn claimed_attributes(&self) -> impl Iterator<Item = (&String, &Attribute)> {
        self.claimed().flat_map(|a| a.attributes.iter())
    }
}

pub(crate) fn recognize(tree: &mut Tree, doc: &mut Document, line: &Line<'_>) {
    let row = line.row;
    for (index, (key, attr)) in line.attributes.list.iter().enumerate() {
        let value = attr.value_span;
        // What the value holds decides how it is painted: a date that reads
        // as one without evaluating, an expression, or plain text.
        let known = doc.attribute_value(key);
        doc.foreign = true;
        match known {
            Some(AttributeValue::When) if syntax::is_relative_date(&attr.value) => {
                tree.paint(value, HighlightKind::Number);
            }
            Some(AttributeValue::Date) if syntax::stamp(&attr.value).is_some() => {
                tree.paint(value, HighlightKind::Number);
            }
            Some(kind) if kind.is_expression() => {
                tree.expression(line.text, row, value.start, value.end);
            }
            _ => tree.paint(value, HighlightKind::String),
        }
        if line.attributes.list[..index].iter().any(|(k, _)| k == key) {
            tree.problem(attr.span, format!("Duplicate @{key} attribute"));
        }
        if known.is_none() {
            tree.problem(attr.span, format!("Unknown attribute @{key}"));
        }
    }
    if let Some(span) = line.attributes.unclosed {
        tree.problem(span, "Unclosed task attribute".into());
    }
    if !line.attributes.map.is_empty() {
        let end = line
            .attributes
            .list
            .iter()
            .map(|(_, a)| a.span.start)
            .min()
            .unwrap_or(line.text.len());
        doc.attributed.push(Attributed {
            line: row,
            on: line.on,
            checkbox: line.checkbox.is_some(),
            title: line.text[line.from.min(end)..end].trim().into(),
            attributes: line.attributes.map.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recognized::Recognizers;

    fn declared(key: &str, value: AttributeValue) -> Arc<Declaration> {
        Arc::new(Declaration {
            key: key.into(),
            module: "m".into(),
            value,
            kinds: vec![],
            params: vec![],
            applies: "attribute".into(),
            documentation: String::new(),
            example: String::new(),
            values: Vec::new(),
            checkbox: false,
        })
    }

    /// A note is read again once modules declare an attribute it writes: the
    /// value is then an expression, whose names are references, and the
    /// line's attributes are live, but for one live only on a checklist item,
    /// which stays prose anywhere else.
    #[test]
    fn declared_attributes_are_known_once_recognized() {
        let text = "- Launch @at(launch)\n- [ ] Pack @due(tomorrow)\nNote @due(x)\n";
        let mut doc = Document::parse(text.into());
        let unknown: Vec<_> = doc.problems.iter().map(|p| p.message.as_str()).collect();
        assert_eq!(
            unknown,
            [
                "Unknown attribute @at",
                "Unknown attribute @due",
                "Unknown attribute @due"
            ]
        );
        assert!(!doc.references.iter().any(|r| r.name == "launch"));
        assert_eq!(doc.claimed().count(), 1);
        let due = Arc::new(Declaration {
            checkbox: true,
            ..(*declared("due", AttributeValue::When)).clone()
        });
        let recognizers = Recognizers {
            attributes: vec![declared("at", AttributeValue::When), due],
            ..Default::default()
        };
        doc.recognize(&recognizers);
        assert!(doc.problems.is_empty());
        assert!(doc.references.iter().any(|r| r.name == "launch"));
        assert_eq!(doc.attribute_value("at"), Some(AttributeValue::When));
        let claimed: Vec<_> = doc.claimed().map(|a| (a.line, a.title.as_str())).collect();
        assert_eq!(claimed, [(0, "Launch"), (1, "Pack")]);
        let live: Vec<_> = doc
            .claimed()
            .flat_map(|a| doc.live_attributes(a).map(|(d, _)| d.key.as_str()))
            .collect();
        assert_eq!(live, ["at", "due"]);
        // Read with the same declarations, it is not parsed again.
        let before = doc.text.as_ptr();
        doc.recognize(&recognizers);
        assert_eq!(doc.text.as_ptr(), before);
    }
}
