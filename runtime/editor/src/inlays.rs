//! Inlays: what providers read and write while contributing inline labels.
use crate::providers;
use lang::eval::RequestContext;
use lang::eval::engine::Engine;
use lang::model::Document;
use lsp_types::{
    InlayHint, InlayHintLabel, InlayHintTooltip, MarkupContent, MarkupKind, Position, Range,
};
use std::path::Path;

/// Every position in a note, for a caller that wants all of its labels.
pub(crate) const FULL_RANGE: Range = Range {
    start: Position {
        line: 0,
        character: 0,
    },
    end: Position {
        line: u32::MAX,
        character: u32::MAX,
    },
};

/// All producers in one request share evaluation memoization and a clock snapshot.
pub(crate) struct InlayContext<'request, 'workspace> {
    pub engine: &'request mut Engine<'workspace>,
    pub path: &'request Path,
    pub document: &'workspace Document,
    pub range: Range,
}
impl InlayContext<'_, '_> {
    /// Call when a label/tooltip reads the clock without going through the evaluator.
    pub(crate) fn mark_time_dependent(&mut self) {
        self.engine.mark_time_dependent(true);
    }
}

/// Owns LSP construction and range filtering so producers only supply content.
pub(crate) struct InlaySink {
    range: Range,
    hints: Vec<InlayHint>,
}
impl InlaySink {
    pub(crate) fn push(&mut self, position: Position, label: String, tooltip: String) {
        if position < self.range.start || position > self.range.end {
            return;
        }
        self.hints.push(InlayHint {
            position,
            label: InlayHintLabel::String(label),
            kind: None,
            text_edits: None,
            tooltip: Some(InlayHintTooltip::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: tooltip,
            })),
            padding_left: Some(true),
            padding_right: None,
            data: None,
        });
    }
}
#[derive(Default)]
pub struct InlayOutput {
    pub hints: Vec<InlayHint>,
    pub time_dependent: bool,
}

/// Run every provider in order, then stably order their output by position.
/// This same path supplies native LSP hints, browser hints, and clock refresh checks.
pub(crate) fn collect(request: &RequestContext<'_>, path: &Path, range: Range) -> InlayOutput {
    let mut engine = request.engine();
    let Some(document) = request.workspace().documents().get(path) else {
        return InlayOutput::default();
    };
    let mut context = InlayContext {
        engine: &mut engine,
        path,
        document,
        range,
    };
    let mut output = InlaySink {
        range,
        hints: Vec::new(),
    };
    providers::inlays(request, &mut context, &mut output);
    output.hints.sort_by_key(|hint| hint.position);
    InlayOutput {
        hints: output.hints,
        time_dependent: engine.time_dependent(),
    }
}

/// Whether any label in the note reads the clock, so a host knows to refresh.
pub(crate) fn live(request: &RequestContext<'_>, path: &Path) -> bool {
    collect(
        request,
        path,
        Range::new(Position::new(0, 0), Position::new(u32::MAX, 0)),
    )
    .time_dependent
}
