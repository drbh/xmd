//! The lookup provider: refreshing the lookups a note reads, whichever
//! module reads them. It offers the refresh on every row whose evaluation
//! read a lookup, or that holds a record a module built that asks for one,
//! and prepares `refresh` for the note or the workspace.
use crate::Request;
use crate::commands::{
    Action, ActionProvider, Ask, Capabilities, CommandId, NOT_MINE, Prepared, PreparedAction,
    Proposal, document_path, titled,
};
use std::{collections::BTreeSet, path::Path};

pub(crate) struct Lookups;
impl ActionProvider for Lookups {
    fn kinds(&self) -> &'static [CommandId] {
        &[CommandId::Refresh]
    }
    fn propose(&self, request: &Request<'_>, path: &Path, ask: Ask) -> Vec<Proposal> {
        let Ok(uri) = lang::common::file_url(path) else {
            return vec![];
        };
        let rows: BTreeSet<usize> = records::lookups(&request.records, &mut request.engine(), path)
            .into_iter()
            .filter_map(|(row, _)| row)
            .filter(|row| ask.rows.has(*row))
            .collect();
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
