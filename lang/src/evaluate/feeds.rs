//! Reading a syndication feed: RSS 2.0, RSS 1.0 and Atom 1.0, in one tolerant
//! scanner over `<tag>…</tag>`.
//!
//! Feeds are the one remote shape a link module cannot decode for itself: the
//! module language splits text but does not parse XML. So the host converts
//! before the module decodes. A `refresh` request that asks for
//! [`RefreshFormat::Feed`](crate::link_features::RefreshFormat) has its program's
//! stdout run through [`parse`], and the module's `decode` sees the ordinary
//! JSON record [`json`] produces.
//!
//! The scanner is deliberately small and forgiving: a feed is other people's
//! XML, so unclosed tags, stray `<`, nested CDATA, HTML inside a description and
//! outright binary all have to end in a value or an error, never a panic.
use chrono::{DateTime, Utc};

/// One feed: the channel's own fields plus its items, newest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Feed {
    pub title: String,
    pub link: Option<String>,
    pub updated: Option<DateTime<Utc>>,
    pub items: Vec<Item>,
}

/// One entry of a feed. Only the title is required; the rest is often absent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub title: String,
    pub link: Option<String>,
    pub published: Option<DateTime<Utc>>,
    pub summary: Option<String>,
}

/// The newest items a feed contributes; longer archives are truncated.
const ITEM_LIMIT: usize = 50;
/// The longest summary kept, so one verbose feed cannot fill the cache.
const SUMMARY_LIMIT: usize = 500;
/// A guard on unterminated text, which is otherwise the whole document.
const TEXT_LIMIT: usize = 64 * 1024;

/// Read a feed document, or say why it is not one.
pub fn parse(xml: &str) -> Result<Feed, String> {
    let mut feed = Feed::default();
    let mut scanner = Scanner { source: xml, at: 0 };
    let mut root = false;
    let mut item: Option<Item> = None;
    let mut field: Option<Field> = None;
    let mut text = String::new();
    while let Some(event) = scanner.next_event() {
        match event {
            Event::Open(name, attributes, closed) => {
                let name = local(&name);
                root |= matches!(name.as_str(), "rss" | "feed" | "rdf" | "channel");
                match name.as_str() {
                    "item" | "entry" => {
                        if let Some(done) = item.replace(Item::default()) {
                            push(&mut feed, done);
                        }
                        field = None;
                        text.clear();
                    }
                    _ => {
                        if field.is_none() && tracked(&name) {
                            field = Some(Field {
                                name: name.clone(),
                                href: attribute(&attributes, "href"),
                                rel: attribute(&attributes, "rel"),
                            });
                            text.clear();
                        }
                    }
                }
                if closed {
                    close(&mut feed, &mut item, &mut field, &mut text, &name);
                }
            }
            Event::Close(name) => {
                let name = local(&name);
                close(&mut feed, &mut item, &mut field, &mut text, &name);
            }
            Event::Text(chunk) => {
                if field.is_some() && text.len() < TEXT_LIMIT {
                    text.push_str(&chunk);
                }
            }
        }
        if feed.items.len() > 10_000 {
            break;
        }
    }
    if let Some(done) = item.take() {
        push(&mut feed, done);
    }
    if !root {
        return Err("Not an RSS or Atom feed".into());
    }
    if feed.title.is_empty() && feed.items.is_empty() {
        return Err("Feed has no title and no items".into());
    }
    feed.items.sort_by(|a, b| match (a.published, b.published) {
        (Some(a), Some(b)) => b.cmp(&a),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    feed.items.truncate(ITEM_LIMIT);
    Ok(feed)
}

/// The JSON a link module's `decode` receives for a feed refresh.
pub fn json(feed: &Feed) -> serde_json::Value {
    serde_json::json!({
        "title": feed.title,
        "link": feed.link,
        "updated": feed.updated.map(stamp),
        "items": feed
            .items
            .iter()
            .map(|item| serde_json::json!({
                "title": item.title,
                "link": item.link,
                "published": item.published.map(stamp),
                "summary": item.summary,
            }))
            .collect::<Vec<_>>(),
    })
}

/// One UTC instant, spelled the way every other module date is.
fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// The element currently collecting text, and the attributes it opened with.
struct Field {
    name: String,
    href: Option<String>,
    rel: Option<String>,
}

/// Elements whose text (or `href`) contributes to a feed or an item.
fn tracked(name: &str) -> bool {
    matches!(
        name,
        "title"
            | "link"
            | "pubdate"
            | "published"
            | "updated"
            | "lastbuilddate"
            | "date"
            | "issued"
            | "modified"
            | "description"
            | "summary"
            | "content"
            | "encoded"
            | "subtitle"
    )
}

/// Finish the open element, if this end tag closes it.
fn close(
    feed: &mut Feed,
    item: &mut Option<Item>,
    field: &mut Option<Field>,
    text: &mut String,
    name: &str,
) {
    if matches!(name, "item" | "entry") {
        *field = None;
        text.clear();
        if let Some(done) = item.take() {
            push(feed, done);
        }
        return;
    }
    let Some(open) = field.take() else { return };
    if open.name != name {
        // A stray end tag inside the element (HTML in a description) is text.
        *field = Some(open);
        return;
    }
    let value = collapse(text);
    text.clear();
    match item {
        Some(item) => assign_item(item, &open, value),
        None => assign_feed(feed, &open, value),
    }
}

/// Record one finished element against the item being read.
fn assign_item(item: &mut Item, open: &Field, value: String) {
    match open.name.as_str() {
        "title" if item.title.is_empty() => item.title = value,
        "link" => {
            if let Some(link) = link_of(open, &value) {
                item.link.get_or_insert(link);
            }
        }
        "pubdate" | "published" | "issued" => {
            if let Some(at) = instant(&value) {
                item.published = Some(at);
            }
        }
        "updated" | "date" | "modified" => {
            if item.published.is_none() {
                item.published = instant(&value);
            }
        }
        "description" | "summary" | "content" | "encoded" => {
            let summary = strip(&value);
            if !summary.is_empty() && item.summary.is_none() {
                item.summary = Some(truncate(summary));
            }
        }
        _ => {}
    }
}

/// Record one finished element against the feed itself.
fn assign_feed(feed: &mut Feed, open: &Field, value: String) {
    match open.name.as_str() {
        "title" if feed.title.is_empty() => feed.title = value,
        "link" => {
            if let Some(link) = link_of(open, &value) {
                feed.link.get_or_insert(link);
            }
        }
        "updated" | "lastbuilddate" | "pubdate" | "date" if feed.updated.is_none() => {
            feed.updated = instant(&value);
        }
        _ => {}
    }
}

/// An Atom `<link href=…>` or an RSS `<link>…</link>`, ignoring other relations.
fn link_of(open: &Field, value: &str) -> Option<String> {
    if let Some(href) = &open.href {
        let rel = open.rel.as_deref().unwrap_or("alternate");
        return (rel == "alternate" && !href.is_empty()).then(|| href.clone());
    }
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// Keep a feed's items and stop reading long archives.
fn push(feed: &mut Feed, item: Item) {
    if !item.title.is_empty() || item.link.is_some() {
        feed.items.push(item);
    }
}

/// Every date spelling feeds use in practice, tried in order.
fn instant(value: &str) -> Option<DateTime<Utc>> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(at) = DateTime::parse_from_rfc2822(value) {
        return Some(at.with_timezone(&Utc));
    }
    if let Ok(at) = DateTime::parse_from_rfc3339(value) {
        return Some(at.with_timezone(&Utc));
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(value, format) {
            return Some(naive.and_utc());
        }
        if let Ok(date) = chrono::NaiveDate::parse_from_str(value, format) {
            return Some(date.and_time(chrono::NaiveTime::MIN).and_utc());
        }
    }
    None
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

/// One space between words, and none at the ends.
fn collapse(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
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

/// Drop a namespace prefix and compare names in lowercase.
fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_lowercase()
}

/// Read one attribute, ignoring the case of its name.
fn attribute(attributes: &[(String, String)], name: &str) -> Option<String> {
    attributes
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

enum Event {
    /// A start tag: name, attributes, and whether it closed itself.
    Open(String, Vec<(String, String)>, bool),
    Close(String),
    Text(String),
}

/// A byte scanner over the document. Every slice is cut at an ASCII delimiter,
/// so it is always a character boundary, whatever the bytes in between are.
struct Scanner<'a> {
    source: &'a str,
    at: usize,
}
impl Scanner<'_> {
    fn next_event(&mut self) -> Option<Event> {
        loop {
            let rest = self.source.get(self.at..)?;
            if rest.is_empty() {
                return None;
            }
            if !rest.starts_with('<') {
                let end = rest.find('<').unwrap_or(rest.len());
                self.at += end;
                let chunk = &rest[..end];
                if chunk.trim().is_empty() {
                    continue;
                }
                return Some(Event::Text(entities(chunk)));
            }
            if let Some(body) = rest.strip_prefix("<![CDATA[") {
                let (text, skipped) = match body.find("]]>") {
                    Some(end) => (&body[..end], end + "]]>".len()),
                    None => (body, body.len()),
                };
                self.at += "<![CDATA[".len() + skipped;
                return Some(Event::Text(text.to_owned()));
            }
            if rest.starts_with("<!--") || rest.starts_with("<?") || rest.starts_with("<!") {
                let (open, close) = if rest.starts_with("<!--") {
                    ("<!--", "-->")
                } else if rest.starts_with("<?") {
                    ("<?", "?>")
                } else {
                    ("<!", ">")
                };
                let body = &rest[open.len()..];
                let end = body.find(close).map_or(body.len(), |i| i + close.len());
                self.at += open.len() + end;
                continue;
            }
            let Some(end) = rest.find('>') else {
                self.at = self.source.len();
                return None;
            };
            let inside = &rest[1..end];
            self.at += end + 1;
            if let Some(name) = inside.strip_prefix('/') {
                return Some(Event::Close(name.trim().to_owned()));
            }
            let closed = inside.ends_with('/');
            let inside = inside.strip_suffix('/').unwrap_or(inside);
            let name_end = inside
                .find(|c: char| c.is_whitespace())
                .unwrap_or(inside.len());
            let name = inside[..name_end].trim().to_owned();
            if name.is_empty() {
                continue;
            }
            return Some(Event::Open(name, attributes(&inside[name_end..]), closed));
        }
    }
}

/// Read `name="value"`, `name='value'` and bare values, skipping anything else.
fn attributes(mut rest: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    while out.len() < 32 {
        rest = rest.trim_start();
        let Some(equals) = rest.find('=') else { break };
        let name = rest[..equals].trim();
        let after = rest[equals + 1..].trim_start();
        let (value, tail) = match after.as_bytes().first() {
            Some(b'"') => split_quoted(&after[1..], '"'),
            Some(b'\'') => split_quoted(&after[1..], '\''),
            _ => {
                let end = after
                    .find(|c: char| c.is_whitespace())
                    .unwrap_or(after.len());
                (&after[..end], &after[end..])
            }
        };
        if !name.is_empty() && !name.contains(char::is_whitespace) {
            out.push((name.to_lowercase(), entities(value)));
        }
        rest = tail;
        if rest.is_empty() {
            break;
        }
    }
    out
}

/// The text up to the closing quote, and what follows it.
fn split_quoted(rest: &str, quote: char) -> (&str, &str) {
    match rest.find(quote) {
        Some(end) => (&rest[..end], &rest[end + 1..]),
        None => (rest, ""),
    }
}

/// Expand the five XML entities and numeric references; leave the rest alone.
fn entities(value: &str) -> String {
    if !value.contains('&') {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find(';').filter(|end| *end <= 12) else {
            out.push('&');
            rest = after;
            continue;
        };
        let name = &after[..end];
        let expanded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => numeric(name),
        };
        match expanded {
            Some(c) => out.push(c),
            None => {
                out.push('&');
                out.push_str(name);
                out.push(';');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// `&#8217;` and `&#x2019;`, when they name a character.
fn numeric(name: &str) -> Option<char> {
    let digits = name.strip_prefix('#')?;
    let code = match digits.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => digits.parse().ok()?,
    };
    char::from_u32(code)
}
