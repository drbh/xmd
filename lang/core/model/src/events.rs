//! Events: a line with an `@at(…)` attribute and no checkbox, titled by the
//! text in front of its first attribute.
use crate::blocks::{Attribute, Line, Tree};
use crate::document::Document;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct Event {
    pub line: usize,
    pub title: String,
    pub attributes: BTreeMap<String, Attribute>,
}

pub(crate) fn recognize(_: &mut Tree, doc: &mut Document, line: &Line<'_>) {
    let attrs = &line.attributes.map;
    if line.checkbox.is_some() || !attrs.contains_key(syntax::AttributeKey::At.as_str()) {
        return;
    }
    let end = attrs
        .values()
        .map(|a| a.span.start)
        .min()
        .unwrap_or(line.text.len());
    doc.events.push(Event {
        line: line.row,
        title: line.text[line.start..end]
            .trim()
            .trim_start_matches("- ")
            .into(),
        attributes: attrs.clone(),
    });
}
