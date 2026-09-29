//! Reading a syndication feed: RSS 2.0, RSS 1.0 and Atom 1.0, via the
//! `feed-rs` crate, thinned to the handful of fields a link module reads.
//!
//! Feeds are the one remote shape a link module cannot decode for itself: the
//! module language splits text but does not parse XML. So the host converts
//! before the module decodes. A `refresh` request that asks for
//! [`RefreshFormat::Feed`](lang::eval::link_features::RefreshFormat) has its program's
//! stdout run through [`parse`], and the module's `decode` sees the ordinary
//! JSON record it produces.
//!
//! `feed-rs` does the actual XML/JSON-feed parsing (entities, CDATA, dates,
//! namespaces); this module only adapts its richer model into the four-field
//! shape the `rss` stdlib module expects, and applies the truncation policy
//! that keeps a cache readable.
use chrono::{DateTime, Utc};
use feed_rs::model as feed_model;
use serde_json::{Value, json};
use std::cmp::Reverse;

/// The newest items a feed contributes; longer archives are truncated.
const ITEM_LIMIT: usize = 50;
/// The longest summary kept, so one verbose feed cannot fill the cache.
const SUMMARY_LIMIT: usize = 500;

/// Read a feed document into the record a link module's `decode` receives:
/// the channel's own fields plus its items, newest first. Or say why it is
/// not a feed.
pub(crate) fn parse(xml: &str) -> Result<Value, String> {
    let parsed = feed_rs::parser::parse(xml.as_bytes())
        .map_err(|_| "Not an RSS or Atom feed".to_string())?;

    let title = parsed.title.as_ref().map(text).unwrap_or_default();
    let link = primary_link(&parsed.links);
    let updated = parsed.updated.or(parsed.published);

    let mut items: Vec<_> = parsed.entries.iter().filter_map(item_of).collect();
    if title.is_empty() && items.is_empty() {
        return Err("Feed has no title and no items".into());
    }
    items.sort_by_key(|(published, _)| Reverse(*published));
    items.truncate(ITEM_LIMIT);
    Ok(json!({
        "title": title,
        "link": link,
        "updated": updated.map(stamp),
        "items": items.into_iter().map(|(_, item)| item).collect::<Vec<_>>(),
    }))
}

/// Adapt one feed-rs entry, with the instant it sorts by, or drop it if it
/// names nothing worth keeping.
fn item_of(entry: &feed_model::Entry) -> Option<(Option<DateTime<Utc>>, Value)> {
    let title = entry.title.as_ref().map(text).unwrap_or_default();
    let link = primary_link(&entry.links);
    if title.is_empty() && link.is_none() {
        return None;
    }
    let published = entry.published.or(entry.updated);
    let item = json!({
        "title": title,
        "link": link,
        "published": published.map(stamp),
        "summary": summary_of(entry),
    });
    Some((published, item))
}

/// The item's own description or summary, preferring `summary`/`description`
/// over `content`/`content:encoded`, with any markup stripped and the result
/// kept short enough for a cache to stay readable.
fn summary_of(entry: &feed_model::Entry) -> Option<String> {
    let raw = entry
        .summary
        .as_ref()
        .map(|t| t.content.as_str())
        .or_else(|| entry.content.as_ref().and_then(|c| c.body.as_deref()))?;
    let stripped = strip(raw);
    (!stripped.is_empty()).then(|| truncate(stripped))
}

/// The one link a feed or item points readers at: an Atom `rel="alternate"`
/// link (the default relation when none is given), or an RSS `<link>`.
fn primary_link(links: &[feed_model::Link]) -> Option<String> {
    links
        .iter()
        .find(|link| {
            link.rel.as_deref().unwrap_or("alternate") == "alternate" && !link.href.is_empty()
        })
        .map(|link| link.href.clone())
}

/// A feed-rs text value, its whitespace collapsed the way a feed indents it.
fn text(value: &feed_model::Text) -> String {
    collapse(&value.content)
}

/// One space between words, and none at the ends.
fn collapse(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Drop markup and collapse the whitespace a feed indents its text with.
fn strip(value: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in value.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    collapse(&out)
}

/// Keep summaries short enough that a cache stays readable.
fn truncate(mut value: String) -> String {
    if value.chars().count() <= SUMMARY_LIMIT {
        return value;
    }
    let cut = value
        .char_indices()
        .nth(SUMMARY_LIMIT)
        .map_or(value.len(), |(i, _)| i);
    value.truncate(cut);
    value.push('…');
    value
}

/// One UTC instant, spelled the way every other module date is.
fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
