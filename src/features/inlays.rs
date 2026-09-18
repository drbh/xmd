//! The common extension point for every inlay, independent of its source or host.
use crate::{document::Document, engine::Engine};
use lsp_types::{
    InlayHint, InlayHintLabel, InlayHintTooltip, MarkupContent, MarkupKind, Position, Range,
};
use std::path::Path;

/// A producer of labels and their tooltips. Collection is read-only: use the
/// request's engine/clock and cached data, never fetch or modify source here.
/// Implement this trait and register the implementation in `inlay_providers::BUILTINS`.
pub trait InlayFeature: Send + Sync {
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink);
}

/// All producers in one request share evaluation memoization and a clock snapshot.
pub struct InlayContext<'request, 'workspace> {
    pub engine: &'request mut Engine<'workspace>,
    pub path: &'request Path,
    pub document: &'workspace Document,
    pub range: Range,
}
impl InlayContext<'_, '_> {
    /// Call when a label/tooltip reads the clock without going through the evaluator.
    pub fn mark_time_dependent(&mut self) {
        self.engine.time_dependent = true;
    }
}

/// Owns LSP construction and range filtering so producers only supply content.
pub struct InlaySink {
    range: Range,
    hints: Vec<InlayHint>,
}
impl InlaySink {
    pub fn push(&mut self, position: Position, label: String, tooltip: String) {
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

/// Run producers in registration order, then stably order their output by position.
/// This same path supplies native LSP hints, browser hints, and clock refresh checks.
pub fn collect(
    engine: &mut Engine<'_>,
    path: &Path,
    range: Range,
    features: &[&dyn InlayFeature],
) -> InlayOutput {
    let Some(document) = engine.workspace.documents.get(path) else {
        return InlayOutput::default();
    };
    let mut context = InlayContext {
        engine,
        path,
        document,
        range,
    };
    let mut output = InlaySink {
        range,
        hints: Vec::new(),
    };
    for feature in features {
        feature.collect(&mut context, &mut output);
    }
    output.hints.sort_by_key(|hint| hint.position);
    InlayOutput {
        hints: output.hints,
        time_dependent: context.engine.time_dependent,
    }
}
