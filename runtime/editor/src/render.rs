//! Standalone HTML from the editor's semantic tokens, inlays, diagnostics and
//! links: the one serializer the CLI's export and the browser share.
use crate::highlighting::{TOKEN_MODIFIERS, TOKEN_TYPES, semantic_tokens};
use crate::links::document_links;
use lang::model::{Document, LineIndex};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, DocumentLink, InlayHint, InlayHintLabel, InlayHintTooltip,
    Position, Range, SemanticToken, TextEdit,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
    path::Path,
};

/// The note at `path` and every inline label the editor shows for it, at one
/// clock snapshot.
fn labelled<'a>(
    request: &crate::Request<'a>,
    path: &Path,
) -> Result<(&'a Document, Vec<InlayHint>), String> {
    let doc = request.workspace().documents().get(path).ok_or_else(|| {
        format!(
            "File is not in this workspace's indexed .{} notes",
            lang::common::EXTENSION
        )
    })?;
    // Every position in the note.
    let everywhere = Range::new(Position::new(0, 0), Position::new(u32::MAX, u32::MAX));
    Ok((
        doc,
        crate::providers::hints(request, path, everywhere).hints,
    ))
}

pub(crate) fn html_for(request: &crate::Request<'_>, path: &Path) -> Result<String, String> {
    let (doc, hints) = labelled(request, path)?;
    let body = fragment(
        doc,
        &request.workspace().prelude_names(path),
        &hints,
        &crate::providers::diagnostics(request, path, false),
        &document_links(request, path),
    )?;
    Ok(document(
        &path.file_name().unwrap_or_default().to_string_lossy(),
        &body,
    ))
}

/// The source with the editor's inline labels written into it.
pub(crate) fn rendered_text(request: &crate::Request<'_>, path: &Path) -> Result<String, String> {
    let (doc, hints) = labelled(request, path)?;
    render_text(&doc.text, &hints)
}

/// Insert display labels, honoring UTF-16 positions, padding and provider order.
/// Inlay text edits are interactive actions and are never applied here.
fn render_text(source: &str, hints: &[InlayHint]) -> Result<String, String> {
    let edits: Vec<_> = hints
        .iter()
        .map(|hint| {
            let mut label = String::new();
            if hint.padding_left == Some(true) {
                label.push(' ');
            }
            label.push_str(&inlay_label(hint));
            TextEdit::new(Range::new(hint.position, hint.position), label)
        })
        .collect();
    lang::model::apply_edits(source, &edits)
}

pub fn fragment(
    doc: &Document,
    library: &[String],
    hints: &[InlayHint],
    diagnostics: &[Diagnostic],
    links: &[DocumentLink],
) -> Result<String, String> {
    serialize(
        &doc.text,
        &semantic_tokens(doc, library),
        hints,
        diagnostics,
        links,
        &line_classes(doc),
    )
}

/// Block styling follows the parser, including its fenced-code/comment context.
pub fn line_classes(doc: &Document) -> Vec<String> {
    let mut lines = vec![String::new(); doc.text.split('\n').count()];
    for section in &doc.sections {
        lines[section.line] = format!("h{}", section.level);
    }
    for task in &doc.tasks {
        lines[task.line] = "task".into();
    }
    lines
}

/// One display line per label, even if its value contains newlines.
fn inlay_label(hint: &InlayHint) -> String {
    let label = match &hint.label {
        InlayHintLabel::String(text) => text.clone(),
        InlayHintLabel::LabelParts(parts) => parts.iter().map(|p| p.value.as_str()).collect(),
    };
    label.replace(['\r', '\n', '\t'], " ")
}

#[derive(Default)]
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

/// Serialize one presentation snapshot. No parsing, evaluation, or source
/// mutations.
fn serialize(
    source: &str,
    tokens: &[SemanticToken],
    hints: &[InlayHint],
    diagnostics: &[Diagnostic],
    links: &[DocumentLink],
    line_classes: &[String],
) -> Result<String, String> {
    let index = LineIndex::new(source);
    let starts = index.starts();
    let offset = |position: Position| -> Result<usize, String> {
        index.offset(position).map_err(|_| {
            if position.line as usize >= starts.len() {
                "Invalid render line".into()
            } else {
                "Invalid UTF-16 render boundary".into()
            }
        })
    };
    let mut events: BTreeMap<usize, Event<'_>> = BTreeMap::new();
    events.entry(0).or_default();
    events.entry(source.len()).or_default();
    for &start in starts {
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
            TOKEN_TYPES
                .get(token.token_type as usize)
                .ok_or("Invalid token type")?
        );
        for (bit, modifier) in TOKEN_MODIFIERS.iter().enumerate() {
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
                ..Decoration::default()
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
                    title: link.tooltip.clone().unwrap_or_default(),
                    href: Some(target.to_string()),
                    ..Decoration::default()
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
fn document(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><style>{}\nhtml {{ background: #202124; }} body {{ margin: 0; padding: 28px; }} @media print {{ html {{ background: white; }} body {{ padding: 0; }} }}</style></head><body><pre class=\"xmd\"><code>{body}</code></pre></body></html>\n",
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
/// the browser loads those files directly, and an export embeds the same ones
/// so it looks identical without a build step.
fn styles() -> String {
    let rules: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../../client/web/theme/palette.json"))
            .expect("valid bundled token palette");
    let mut css = include_str!("../../../client/web/theme/base.css").to_string();
    for rule in rules {
        write!(css, ".xmd .t-{}", rule["token_type"].as_str().unwrap()).unwrap();
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
