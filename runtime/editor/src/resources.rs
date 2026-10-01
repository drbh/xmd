//! The resource provider: opening and refreshing what a row points at. A row
//! offers a resource for each link on it, each definition on it whose value
//! is a resource, and each reference on it to a name whose value is one.
//! The note is indexed by row in one pass, so proposing for every row costs
//! one walk over its definitions, references and links.
use crate::Request;
use crate::commands::{
    Action, ActionProvider, Ask, Capabilities, CommandId, NOT_MINE, Prepared, PreparedAction,
    Proposal, RowTarget, Rows,
};
use lang::eval::engine::Value;
use lang::eval::resources::{Resource, ResourcePresenting};
use lang::model::Document;
use lang::stdlib;
use std::{collections::BTreeMap, path::Path};

pub(crate) struct Resources;
impl ActionProvider for Resources {
    fn kinds(&self) -> &'static [CommandId] {
        &[CommandId::OpenResource, CommandId::RefreshResource]
    }
    /// Open, and refresh where the resource's link feature can, for each
    /// resource on a row, in URL order.
    fn propose(&self, request: &Request<'_>, path: &Path, ask: Ask) -> Vec<Proposal> {
        let Some(doc) = request.workspace().documents().get(path) else {
            return vec![];
        };
        let Ok(uri) = lang::common::file_url(path) else {
            return vec![];
        };
        let mut proposals = vec![];
        let mut engine = request.engine();
        for (row, found) in by_row(request, path, doc, ask.rows) {
            let target = RowTarget::at(&uri, doc, row);
            for (url, resource) in found {
                let title = stdlib::shown(stdlib::resource::control(
                    &mut engine,
                    resource.record(path),
                ));
                proposals.push(Proposal {
                    line: row,
                    title,
                    action: Action::OpenResource {
                        target: target.clone(),
                        url: url.clone(),
                    },
                });
                if let Some(refresh) = request.link_features().refresh_request(&resource.target) {
                    proposals.push(Proposal {
                        line: row,
                        title: refresh.title,
                        action: Action::RefreshResource {
                            target: target.clone(),
                            url,
                        },
                    });
                }
            }
        }
        proposals
    }
    fn prepare(
        &self,
        request: &Request<'_>,
        action: &Action,
        _: Capabilities,
    ) -> Result<Prepared, String> {
        let (Action::OpenResource { target, url } | Action::RefreshResource { target, url }) =
            action
        else {
            return Err(NOT_MINE.into());
        };
        let (path, doc) = target.validate(request)?;
        let resource = by_row(request, &path, doc, Rows::One(target.row))
            .remove(&target.row)
            .and_then(|mut found| found.remove(url))
            .ok_or("Resource changed; request fresh controls")?;
        if matches!(action, Action::OpenResource { .. }) {
            Ok(PreparedAction::Open { url: url.clone() }.into())
        } else if request
            .link_features()
            .refresh_request(&resource.target)
            .is_some()
        {
            Ok(PreparedAction::RefreshResource { resource }.into())
        } else {
            Err("This resource does not support refresh".into())
        }
    }
}

/// The resources on each of `rows`, by the URL each opens. Where two name
/// the same URL, the later one (definitions, then references, then links)
/// is the one kept.
fn by_row(
    request: &Request<'_>,
    path: &Path,
    doc: &Document,
    rows: Rows,
) -> BTreeMap<usize, BTreeMap<url::Url, Resource>> {
    // The names each row reads, in the order they are resolved.
    let mut names: BTreeMap<usize, Vec<&str>> = BTreeMap::new();
    let defined = doc
        .definitions
        .iter()
        .map(|d| (d.named.span.line, &d.named.name));
    let read = doc.references.iter().map(|r| (r.span.line, &r.name));
    for (row, name) in defined.chain(read).filter(|(row, _)| rows.has(*row)) {
        names.entry(row).or_default().push(name);
    }
    let mut found: BTreeMap<usize, BTreeMap<url::Url, Resource>> = BTreeMap::new();
    let mut add = |row: usize, resource: Resource| {
        if let Ok(url) = resource.url(path) {
            found.entry(row).or_default().insert(url, resource);
        }
    };
    for (row, names) in names {
        let mut engine = request.engine();
        for name in names {
            if let Ok(Value::Resource(r)) = engine.named(path, name) {
                add(row, r);
            }
        }
    }
    for link in doc.links.iter().filter(|l| rows.has(l.span.line)) {
        if let Some(mut r) = Resource::parse(&link.target) {
            r.origin = Some(path.into());
            add(link.span.line, r);
        }
    }
    found
}
