//! Standalone HTML from the editor's semantic tokens, inlays, diagnostics and links.
use crate::{
    RequestContext,
    document::{Document, byte_at},
    presentation,
};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, DocumentLink, InlayHint, InlayHintTooltip, Position, Range,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
    path::Path,
};

pub fn html_in(request: &RequestContext<'_>, path: &Path) -> Result<String, String> {
    let doc = request
        .workspace()
        .documents
        .get(path)
        .ok_or("File is not in this workspace's indexed .wtf notes")?;
    let hints = presentation::hints_in(
        request,
        path,
        Range::new(Position::new(0, 0), Position::new(u32::MAX, u32::MAX)),
    );
    html(
        &path.file_name().unwrap_or_default().to_string_lossy(),
        doc,
        &hints.hints,
        &crate::diagnostics::collect_in(request, path, false),
        &presentation::document_links_in(request, path),
    )
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

/// Render an existing editor snapshot without evaluating or applying any actions.
pub fn html(
    title: &str,
    doc: &Document,
    hints: &[InlayHint],
    diagnostics: &[Diagnostic],
    links: &[DocumentLink],
) -> Result<String, String> {
    let lines: Vec<_> = doc.text.split('\n').collect();
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
    events.entry(doc.text.len()).or_default();
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
    for token in presentation::semantic_tokens(doc) {
        row += token.delta_line;
        column = if token.delta_line == 0 {
            column + token.delta_start
        } else {
            token.delta_start
        };
        let mut class = format!("t-{}", presentation::TOKEN_TYPES[token.token_type as usize]);
        for (bit, modifier) in presentation::TOKEN_MODIFIERS.iter().enumerate() {
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
            && matches!(
                target.scheme(),
                "https" | "http" | "mailto" | "geo" | "file"
            )
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
    while let Some((start, event)) = events.next() {
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
            if hint.padding_left == Some(true) {
                body.push(' ');
            }
            let title = match &hint.tooltip {
                Some(InlayHintTooltip::String(s)) => s.as_str(),
                Some(InlayHintTooltip::MarkupContent(m)) => m.value.as_str(),
                None => "",
            };
            write!(
                body,
                "<span class=\"inlay\" title=\"{}\">{}</span>",
                escape(title),
                escape(&presentation::inlay_label(hint))
            )
            .unwrap();
            if hint.padding_right == Some(true) {
                body.push(' ');
            }
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
        let text = escape(&doc.text[start..*end]);
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
    Ok(format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{}</title>\n<style>\n{}\n{}\n</style>\n</head>\n<body><pre class=\"wtf\"><code>{body}</code></pre></body>\n</html>\n",
        escape(title),
        token_styles(),
        include_str!("rendering.css")
    ))
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
fn token_styles() -> String {
    let rules: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../ide/zed/languages/wtf/semantic_token_rules.json"
    ))
    .expect("valid bundled token palette");
    let mut css = String::new();
    for rule in rules {
        write!(css, ".t-{}", rule["token_type"].as_str().unwrap()).unwrap();
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
    css
}
