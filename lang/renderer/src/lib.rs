//! Canonical HTML serializer shared by the CLI and browser host.

use lsp_types::{
    Diagnostic, DiagnosticSeverity, DocumentLink, InlayHint, InlayHintLabel, InlayHintTooltip,
    Position, Range, SemanticToken,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

pub struct Snapshot<'a> {
    pub source: &'a str,
    pub tokens: &'a [SemanticToken],
    pub token_types: &'a [&'a str],
    pub token_modifiers: &'a [&'a str],
    pub hints: &'a [InlayHint],
    pub diagnostics: &'a [Diagnostic],
    pub links: &'a [DocumentLink],
    pub line_classes: &'a [String],
}

fn byte_at(line: &str, character: u32) -> Option<usize> {
    let mut units = 0;
    for (byte, c) in line.char_indices() {
        if units == character {
            return Some(byte);
        }
        units += c.len_utf16() as u32;
        if units > character {
            return None;
        }
    }
    (units == character).then_some(line.len())
}
fn inlay_label(hint: &InlayHint) -> String {
    let label = match &hint.label {
        InlayHintLabel::String(s) => s.clone(),
        InlayHintLabel::LabelParts(parts) => parts.iter().map(|part| part.value.as_str()).collect(),
    };
    label.replace(['\r', '\n', '\t'], " ")
}

struct Decoration {
    class: String,
    title: String,
    href: Option<String>,
}
#[derive(Default)]
struct Event<'a> {
    start: Vec<usize>,
    end: Vec<usize>,
    points: Vec<usize>,
    hints: Vec<&'a InlayHint>,
}

/// Render a presentation snapshot. No parsing, evaluation, or source mutations.
pub fn fragment(snapshot: &Snapshot<'_>) -> Result<String, String> {
    let Snapshot {
        source,
        tokens,
        token_types,
        token_modifiers,
        hints,
        diagnostics,
        links,
        line_classes,
    } = *snapshot;
    let lines: Vec<_> = source.split('\n').collect();
    let mut base = 0;
    let starts: Vec<_> = lines
        .iter()
        .map(|line| {
            let start = base;
            base += line.len() + 1;
            start
        })
        .collect();
    let offset = |position: Position| -> Result<usize, String> {
        let row = position.line as usize;
        let line = lines
            .get(row)
            .ok_or("Invalid render line")?
            .trim_end_matches('\r');
        byte_at(line, position.character)
            .map(|column| starts[row] + column)
            .ok_or("Invalid UTF-16 render boundary".into())
    };
    let mut events: BTreeMap<usize, Event<'_>> = BTreeMap::new();
    events.entry(0).or_default();
    events.entry(source.len()).or_default();
    for &start in &starts {
        events.entry(start).or_default();
    }
    let mut decorations = vec![];
    let mut decorate = |range: Range, decoration: Decoration| -> Result<(), String> {
        let (start, end) = (offset(range.start)?, offset(range.end)?);
        if start > end {
            return Err("Reversed render range".into());
        }
        if start == end && decoration.class.starts_with("diagnostic") {
            events
                .entry(start)
                .or_default()
                .points
                .push(decorations.len());
            decorations.push(decoration);
        } else if start != end {
            let index = decorations.len();
            decorations.push(decoration);
            events.entry(start).or_default().start.push(index);
            events.entry(end).or_default().end.push(index);
        }
        Ok(())
    };
    let (mut row, mut column) = (0, 0);
    for token in tokens {
        row += token.delta_line;
        column = if token.delta_line == 0 {
            column + token.delta_start
        } else {
            token.delta_start
        };
        let mut class = format!(
            "t-{}",
            token_types
                .get(token.token_type as usize)
                .ok_or("Invalid token type")?
        );
        for (bit, modifier) in token_modifiers.iter().enumerate() {
            if token.token_modifiers_bitset & (1 << bit) != 0 {
                write!(class, " {modifier}").unwrap();
            }
        }
        decorate(
            Range::new(
                Position::new(row, column),
                Position::new(row, column + token.length),
            ),
            Decoration {
                class,
                title: String::new(),
                href: None,
            },
        )?;
    }
    for diagnostic in diagnostics {
        let kind = match diagnostic.severity {
            Some(DiagnosticSeverity::WARNING) => "warning",
            Some(DiagnosticSeverity::INFORMATION) => "info",
            Some(DiagnosticSeverity::HINT) => "hint",
            _ => "error",
        };
        decorate(
            diagnostic.range,
            Decoration {
                class: format!("diagnostic {kind}"),
                title: diagnostic.message.clone(),
                href: None,
            },
        )?;
    }
    for link in links {
        if let Some(target) = &link.target
            && target
                .scheme()
                .is_some_and(|s| matches!(s.as_str(), "https" | "http" | "mailto" | "geo" | "file"))
        {
            decorate(
                link.range,
                Decoration {
                    class: String::new(),
                    title: link.tooltip.clone().unwrap_or_default(),
                    href: Some(target.to_string()),
                },
            )?;
        }
    }
    for hint in hints {
        events
            .entry(offset(hint.position)?)
            .or_default()
            .hints
            .push(hint);
    }
    let mut body = String::new();
    let mut active = BTreeSet::new();
    let mut events = events.into_iter().peekable();
    let mut line_open = false;
    while let Some((start, event)) = events.next() {
        if let Ok(line) = starts.binary_search(&start) {
            if line_open {
                body.push_str("</span>");
            }
            write!(
                body,
                "<span class=\"line {}\" data-line=\"{line}\">",
                escape(line_classes.get(line).map(String::as_str).unwrap_or(""))
            )
            .unwrap();
            line_open = true;
        }
        for index in event.end {
            active.remove(&index);
        }
        active.extend(event.start);
        for index in event.points {
            let decoration = &decorations[index];
            write!(
                body,
                "<span class=\"{} point\" title=\"{}\"></span>",
                escape(&decoration.class),
                escape(&decoration.title)
            )
            .unwrap();
        }
        for hint in event.hints {
            let title = match &hint.tooltip {
                Some(InlayHintTooltip::String(s)) => s.as_str(),
                Some(InlayHintTooltip::MarkupContent(m)) => m.value.as_str(),
                None => "",
            };
            write!(
                body,
                "<span class=\"inlay\" contenteditable=\"false\" title=\"{}\">{}{}</span>",
                escape(title),
                if hint.padding_left == Some(true) {
                    " "
                } else {
                    ""
                },
                escape(&inlay_label(hint))
                    + if hint.padding_right == Some(true) {
                        " "
                    } else {
                        ""
                    }
            )
            .unwrap();
        }
        let Some((end, _)) = events.peek() else {
            break;
        };
        let selected: Vec<_> = active.iter().map(|i| &decorations[*i]).collect();
        let class = selected
            .iter()
            .map(|d| d.class.as_str())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let title = selected
            .iter()
            .map(|d| d.title.as_str())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        let href = selected.iter().find_map(|d| d.href.as_deref());
        let text = escape(&source[start..*end]);
        if let Some(href) = href {
            write!(
                body,
                "<a class=\"{}\" title=\"{}\" href=\"{}\">{text}</a>",
                escape(&class),
                escape(&title),
                escape(href)
            )
            .unwrap();
        } else if !class.is_empty() || !title.is_empty() {
            write!(
                body,
                "<span class=\"{}\" title=\"{}\">{text}</span>",
                escape(&class),
                escape(&title)
            )
            .unwrap();
        } else {
            body.push_str(&text);
        }
    }
    if line_open {
        body.push_str("</span>");
    }
    Ok(body)
}

/// A standalone page has no scripts, fonts, or other external requests.
pub fn document(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><style>{}\nhtml {{ background: #202124; }} body {{ margin: 0; padding: 28px; }} @media print {{ html {{ background: white; }} body {{ padding: 0; }} }}</style></head><body><pre class=\"wtf\"><code>{body}</code></pre></body></html>\n",
        escape(title),
        styles()
    )
}

fn escape(text: &str) -> String {
    let mut output = String::new();
    for c in text.chars() {
        match c {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(c),
        }
    }
    output
}

/// Use the shipped editor palette instead of maintaining a second token-color map.
///
/// The theme is part of the web client (`client/web/theme/`), not of this crate:
/// the browser loads those files directly, and the renderer embeds the same ones
/// so a standalone HTML export looks identical without a build step.
pub fn styles() -> String {
    let rules: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../../client/web/theme/palette.json"))
            .expect("valid bundled token palette");
    let mut css = include_str!("../../../client/web/theme/base.css").to_string();
    for rule in rules {
        write!(css, ".wtf .t-{}", rule["token_type"].as_str().unwrap()).unwrap();
        if let Some(modifiers) = rule["token_modifiers"].as_array() {
            for modifier in modifiers {
                write!(css, ".{}", modifier.as_str().unwrap()).unwrap();
            }
        }
        write!(
            css,
            " {{ color: {};",
            rule["foreground_color"].as_str().unwrap()
        )
        .unwrap();
        for (key, property) in [("font_weight", "font-weight"), ("font_style", "font-style")] {
            if let Some(value) = rule[key].as_str() {
                write!(css, " {property}: {value};").unwrap();
            }
        }
        if rule["underline"] == true {
            css.push_str(" text-decoration-line: underline;");
        }
        if rule["strikethrough"] == true {
            css.push_str(" text-decoration-line: line-through;");
        }
        css.push_str(" }\n");
    }
    css.push_str(include_str!("../../../client/web/theme/print.css"));
    css
}
