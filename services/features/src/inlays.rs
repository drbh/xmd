//! Inlays: what providers read and write while contributing inline labels.
use lang::document::Document;
use lang::eval::engine::Engine;
use lsp_types::{InlayHint, InlayHintLabel, InlayHintTooltip, Position, Range};
use std::path::Path;

/// All producers in one request share evaluation memoization and a clock snapshot.
pub(crate) struct InlayContext<'request, 'workspace> {
    pub request: &'request crate::Request<'workspace>,
    pub engine: &'request mut Engine<'workspace>,
    pub path: &'request Path,
    pub document: &'workspace Document,
    pub range: Range,
    /// Whether the labels are wanted, or only whether any of them moves with
    /// the clock.
    pub labels: bool,
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
            tooltip: Some(InlayHintTooltip::MarkupContent(analysis::markup(tooltip))),
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

/// Run `providers` over one context and sink, then stably order their output
/// by position. With `labels` off, only whether any label moves with the
/// clock is wanted. `providers` decides who contributes, so this same path
/// supplies native LSP hints, browser hints and clock refresh checks.
pub(crate) fn run(
    request: &crate::Request<'_>,
    path: &Path,
    range: Range,
    labels: bool,
    providers: impl FnOnce(&mut InlayContext<'_, '_>, &mut InlaySink),
) -> InlayOutput {
    let mut engine = request.engine();
    let Some(document) = request.workspace().documents().get(path) else {
        return InlayOutput::default();
    };
    let mut context = InlayContext {
        request,
        engine: &mut engine,
        path,
        document,
        range,
        labels,
    };
    let mut output = InlaySink {
        range,
        hints: Vec::new(),
    };
    providers(&mut context, &mut output);
    output.hints.sort_by_key(|hint| hint.position);
    InlayOutput {
        hints: output.hints,
        time_dependent: engine.time_dependent(),
    }
}
