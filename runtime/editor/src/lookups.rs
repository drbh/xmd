//! The lookup provider: refreshing the rates, quotes and forecasts a note
//! reads. It offers the refresh on every row that calls a lookup or plans a
//! day at a place, and prepares `refresh` for the note or the workspace.
use crate::Request;
use crate::commands::{
    Action, ActionProvider, Ask, Capabilities, CommandId, NOT_MINE, Prepared, PreparedAction,
    Proposal, document_path, titled,
};
use std::{collections::BTreeSet, path::Path};

/// The calls that read a lookup.
const LOOKUPS: [&str; 5] = ["rate(", "to(", "forecast(", "forecast_range(", "quote("];

pub(crate) struct Lookups;
impl ActionProvider for Lookups {
    fn kinds(&self) -> &'static [CommandId] {
        &[CommandId::Refresh]
    }
    fn propose(&self, request: &Request<'_>, path: &Path, ask: Ask) -> Vec<Proposal> {
        let Some(doc) = request.workspace().documents().get(path) else {
            return vec![];
        };
        let Ok(uri) = lang::common::file_url(path) else {
            return vec![];
        };
        let places: BTreeSet<usize> = doc
            .days
            .iter()
            .filter(|d| d.places.is_some())
            .map(|d| d.line)
            .collect();
        let rows = (0..doc.text.lines().count())
            .filter(|row| ask.rows.has(*row))
            .filter(|row| {
                let line = doc.line(*row);
                LOOKUPS.iter().any(|call| line.contains(call)) || places.contains(row)
            })
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return vec![];
        }
        let title = titled(&mut request.engine(), "refresh", "lookups");
        rows.into_iter()
            .map(|line| Proposal {
                line,
                title: title.clone(),
                action: Action::Refresh {
                    document: Some(uri.clone()),
                },
            })
            .collect()
    }
    fn prepare(
        &self,
        _: &Request<'_>,
        action: &Action,
        _: Capabilities,
    ) -> Result<Prepared, String> {
        let Action::Refresh { document } = action else {
            return Err(NOT_MINE.into());
        };
        Ok(PreparedAction::Refresh {
            path: document.as_ref().map(document_path).transpose()?,
        }
        .into())
    }
}
