//! The checks of what a feature module's manifest declares: its
//! `collections`, `attributes`, `forms` and `recognizes`, and the field
//! readers they share with `Module::compile`.
use document::Declaration;
use document::forms::{Column, Form, Reading, Unknowns};
use document::recognized::{Brush, On, Paint, Rule, Term};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use values::{Collection, EvalError, EvalResult, FromValue, Value};

use crate::module::{Declared, Joins};

/// The most recognizers, collections, attributes or forms one module
/// declares, of each.
const MAX_DECLARED: usize = 16;

/// `collections: {name: {entries?, from?}}`: each name one no native
/// collection has. Whether another module declares it too is the registry's check.
pub(crate) fn collections(declared: &Value) -> EvalResult<Vec<Declared>> {
    let keys = "collection names";
    entries(declared, "collections", keys, |name, entry| {
        let Ok(Collection::Declared(name)) = name.parse::<Collection>() else {
            return Err(format!(
                "collections cannot declare '{name}': it is not a free collection name"
            )
            .into());
        };
        let entry = Entry::new(format!("collections.{name}"), entry, &["entries", "from"])?;
        let from = match entry.get("from") {
            None => BTreeMap::new(),
            Some(Value::Record(selections)) => selections
                .iter()
                .map(|(input, kept)| Ok((input.parse()?, Some(strings(kept)?))))
                .collect::<EvalResult<_>>()?,
            Some(names) => strings(names)?
                .iter()
                .map(|input| Ok((input.parse()?, None)))
                .collect::<EvalResult<_>>()?,
        };
        let entries = match entry.get("entries") {
            None | Some(Value::Bool(false)) => Joins::None,
            Some(Value::Bool(true)) => Joins::All,
            Some(Value::Text(field)) if document::identifier(field) => {
                Joins::Where(field.as_str().into())
            }
            Some(_) => return Err(entry.must("entries", "be true, false or a field name")),
        };
        Ok(Declared {
            name,
            entries,
            from,
        })
    })
}

/// `attributes: {key: {value, params?, applies?, doc?, example?, values?,
/// on?}}`.
/// Whether another module declares the key too is the registry's check.
pub(crate) fn attributes(module: &str, declared: &Value) -> EvalResult<Vec<Arc<Declaration>>> {
    entries(declared, "attributes", "attribute keys", |key, entry| {
        if !document::identifier(key) {
            return Err(format!("Invalid attribute key '{key}'").into());
        }
        let known = [
            "value", "params", "applies", "doc", "example", "values", "on",
        ];
        let entry = Entry::new(format!("attributes.{key}"), entry, &known)?;
        let (value, kinds) = match entry.get("value") {
            Some(Value::Text(value)) => {
                syntax::AttributeValue::declared(value).map(|value| (value, vec![]))
            }
            Some(Value::Record(tagged)) if tagged.len() == 1 => tagged
                .get("tagged")
                .and_then(|kinds| strings(kinds).ok())
                .filter(|kinds| {
                    !kinds.is_empty() && kinds.iter().all(|k| common::ValueType::taggable(k))
                })
                .map(|kinds| (syntax::AttributeValue::Tagged, kinds)),
            _ => None,
        }
        .ok_or_else(|| {
            let names = syntax::AttributeValue::NAMES.join(", ");
            let tagged = "{tagged: [kinds]} naming the kinds of tagged record it takes";
            entry.must("value", &format!("be one of {names}, or {tagged}"))
        })?;
        Ok(Arc::new(Declaration {
            key: key.clone(),
            module: module.into(),
            value,
            kinds,
            params: entry.list("params")?.unwrap_or_default(),
            values: entry.list("values")?.unwrap_or_default(),
            checkbox: entry.one_of("on", &[("checkbox", true), ("any", false)], Some(false))?,
            applies: entry.text("applies", "attribute")?,
            documentation: entry.text("doc", "")?,
            example: entry.text("example", "")?,
        }))
    })
}

/// How the host reads one argument or column of a form, `at` naming it for
/// the problem; a `name` only in a table.
fn reading(at: &str, value: Option<&Value>, table: bool) -> EvalResult<Reading> {
    match value {
        Some(Value::Text(reads)) => Reading::declared(reads),
        _ => None,
    }
    .filter(|reading| table || *reading != Reading::Name)
    .ok_or_else(|| {
        let names = ["linear or constraint", "linear, constraint or name"][table as usize];
        format!("{at} must be {names}").into()
    })
}

/// One column of a form's table: `{name, reads, example?}`.
fn column(at: &str, column: &Value) -> EvalResult<Column> {
    let Value::Record(column) = column else {
        return Err(format!("{at} must list its columns as {{name, reads, example?}}").into());
    };
    if let Some(field) = unknown(column, &["name", "reads", "example"]) {
        return Err(format!("{at} columns have no field '{field}'").into());
    }
    let text = |field: &str| match column.get(field) {
        None => Ok(String::new()),
        Some(Value::Text(text)) => Ok(text.clone()),
        Some(_) => Err(format!("{at} column {field}s must be text")),
    };
    let name = text("name")?;
    if !document::identifier(&name) {
        return Err(format!("{at} columns need an identifier name").into());
    }
    Ok(Column {
        reads: reading(&format!("{at}.{name}.reads"), column.get("reads"), true)?,
        name,
        example: text("example")?,
    })
}

/// `forms: {name: {params, reads, table?, unknowns, unknown?, noun?,
/// returns?, doc?, example?}}`. Whether another module declares the name
/// too is the registry's check.
pub(crate) fn forms(module: &str, declared: &Value) -> EvalResult<Vec<Arc<Form>>> {
    entries(declared, "forms", "form names", |name, entry| {
        if !document::identifier(name) || syntax::is_builtin_function(name) {
            return Err(format!(
                "forms cannot declare '{name}': a form is named by an identifier no built-in has"
            )
            .into());
        }
        let known = [
            "params", "reads", "table", "unknowns", "unknown", "noun", "returns", "doc", "example",
        ];
        let entry = Entry::new(format!("forms.{name}"), entry, &known)?;
        let params =
            (entry.list("params")?).ok_or_else(|| entry.must("params", "be a list of text"))?;
        let reads = match entry.get("reads") {
            Some(Value::List(items)) if items.len() == params.len() => items
                .iter()
                .map(|item| reading(&format!("forms.{name}.reads"), Some(item), false))
                .collect::<EvalResult<Vec<_>>>()?,
            _ => return Err(entry.must("reads", "list how each of its params is read")),
        };
        let at = format!("forms.{name}.table");
        let table = match entry.get("table") {
            None => vec![],
            Some(Value::List(columns)) if !columns.is_empty() => columns
                .iter()
                .map(|c| column(&at, c))
                .collect::<EvalResult<Vec<_>>>()?,
            Some(_) => {
                return Err(entry.must("table", "list its columns as {name, reads, example?}"));
            }
        };
        if table.iter().filter(|c| c.reads == Reading::Name).count() > 1 {
            return Err(format!("{at} names its rows in one column at most").into());
        }
        let choices = [("free", Unknowns::Free), ("own", Unknowns::Own)];
        let unknowns = entry.one_of("unknowns", &choices, None)?;
        let (unknown, unknown_doc) = match entry.get("unknown") {
            None => Some(("unknown".to_owned(), String::new())),
            Some(Value::Record(described)) if unknown(described, &["name", "doc"]).is_none() => {
                match (described.get("name"), described.get("doc")) {
                    (Some(Value::Text(n)), Some(Value::Text(d))) => Some((n.clone(), d.clone())),
                    (Some(Value::Text(n)), None) => Some((n.clone(), String::new())),
                    _ => None,
                }
            }
            Some(_) => None,
        }
        .ok_or_else(|| entry.must("unknown", "be {name, doc?}"))?;
        Ok(Arc::new(Form {
            name: name.clone(),
            module: module.into(),
            params,
            reads,
            table,
            unknowns,
            unknown,
            unknown_doc,
            noun: entry.text("noun", name)?,
            returns: entry.text("returns", "")?,
            documentation: entry.text("doc", "")?,
            example: entry.text("example", "")?,
        }))
    })
}

/// `recognizes: [{name, on, pattern, unless?, under?, until?, terms?,
/// tokens?, links?}]`, each pattern compiled and every group a field names
/// checked against the pattern's named groups.
pub(crate) fn rules(module: &str, declared: &Value) -> EvalResult<Vec<Arc<Rule>>> {
    let Value::List(items) = declared else {
        return Err("recognizes must be a list of records".into());
    };
    if items.len() > MAX_DECLARED {
        return Err(format!("A module declares at most {MAX_DECLARED} recognizers").into());
    }
    let mut names = BTreeSet::new();
    let mut rules = Vec::new();
    for item in items.iter() {
        let Value::Record(fields) = item else {
            return Err("recognizes must be a list of records".into());
        };
        let known = [
            "name", "on", "pattern", "unless", "under", "until", "terms", "tokens", "links",
            "title", "record",
        ];
        if let Some(key) = unknown(fields, &known) {
            return Err(format!("Unknown recognizer field '{key}'").into());
        }
        let text = |key: &str| match fields.get(key) {
            Some(Value::Text(text)) => Ok(text.clone()),
            _ => Err(format!("Each recognizer needs {key} as text")),
        };
        let name = text("name")?;
        if !document::identifier(&name) {
            return Err(format!("Invalid recognizer name '{name}'").into());
        }
        if !names.insert(name.clone()) {
            return Err(format!("Duplicate recognizer '{name}'").into());
        }
        let fail = |message: &str| EvalError::from(format!("Recognizer '{name}': {message}"));
        let on: On = text("on")?
            .parse()
            .map_err(|_| fail("on must be prose, item, heading, row or line"))?;
        let compile = |source: &str| {
            common::Pattern::new(source)
                .map(Arc::new)
                .map_err(|e| fail(&e))
        };
        let mut rule = Rule::new(module, &name, on, compile(&text("pattern")?)?);
        let optional = |key: &str| match fields.get(key) {
            None => Ok(None),
            Some(Value::Text(text)) => Ok(Some(text.clone())),
            Some(_) => Err(fail(&format!("{key} must be text"))),
        };
        rule.unless = optional("unless")?.as_deref().map(compile).transpose()?;
        rule.under = optional("under")?;
        rule.until = optional("until")?
            .map(|until| {
                until
                    .parse()
                    .map_err(|_| fail("until must be heading or break"))
            })
            .transpose()?;
        if on != On::Line && (rule.under.is_some() || rule.until.is_some()) {
            return Err(fail("only a line recognizer has under or until"));
        }
        rule.title =
            flag(fields.get("title"), false).ok_or_else(|| fail("title must be true or false"))?;
        rule.record =
            flag(fields.get("record"), true).ok_or_else(|| fail("record must be true or false"))?;
        if !rule.record && (rule.under.is_some() || rule.until.is_some()) {
            return Err(fail("a recognizer that only paints has no under or until"));
        }
        let group = |group: &str, field: &str| {
            if rule.pattern.group_names().any(|g| g == group) {
                Ok(group.to_owned())
            } else if field == "tokens" {
                let what = "which its pattern does not name";
                Err(format!("Recognizer '{name}' paints '{group}', {what}").into())
            } else {
                Err(fail(&format!(
                    "{field} names '{group}', which its pattern does not name"
                )))
            }
        };
        let record = |key: &str| match fields.get(key) {
            None => Ok(None),
            Some(Value::Record(fields)) => Ok(Some(fields.clone())),
            Some(_) => Err(fail(&format!("{key} must be a record"))),
        };
        let mut terms = Vec::new();
        for (name, table) in record("terms")?.iter().flat_map(|r| r.iter()) {
            terms.push((
                group(name, "terms")?,
                term_table(table).map_err(|e| fail(&e))?,
            ));
        }
        let mut tokens = Vec::new();
        for (name, brush) in record("tokens")?.iter().flat_map(|r| r.iter()) {
            let brush = paint_brush(name, brush).map_err(|e| fail(&e))?;
            for by in &brush.terms {
                group(by, "tokens")?;
            }
            tokens.push((group(name, "tokens")?, brush));
        }
        let mut links = Vec::new();
        for (name, template) in record("links")?.iter().flat_map(|r| r.iter()) {
            match template {
                Value::Text(url) if url.contains("{}") => {
                    links.push((group(name, "links")?, url.clone()));
                }
                _ => return Err(fail("a link is a URL with {} for the text")),
            }
        }
        rule.terms = terms;
        rule.tokens = tokens;
        rule.links = links;
        rules.push(rule);
    }
    // A rule goes under another line rule of the module that stays open.
    for rule in &rules {
        if let Some(under) = &rule.under
            && !rules
                .iter()
                .any(|r| r.name == *under && r.name != rule.name && r.until.is_some())
        {
            return Err(format!(
                "Recognizer '{}' is under '{under}', which is not another recognizer with until",
                rule.name
            )
            .into());
        }
    }
    Ok(rules.into_iter().map(Arc::new).collect())
}

/// A group's terms: `[[text, term], ...]` in order, or a record of text to
/// term.
fn term_table(table: &Value) -> Result<Vec<Term>, String> {
    let pair = |text: &Value, term: &Value| match (text, term) {
        (Value::Text(text), Value::Text(term)) => Ok(Term::new(text.clone(), term)),
        _ => Err("terms are text".to_owned()),
    };
    match table {
        Value::List(pairs) => pairs
            .iter()
            .map(|entry| match entry {
                Value::List(pair_) if pair_.len() == 2 => pair(&pair_[0], &pair_[1]),
                _ => Err("terms are [text, term] pairs".to_owned()),
            })
            .collect(),
        Value::Record(fields) => fields
            .iter()
            .map(|(text, term)| pair(&Value::Text(text.clone()), term))
            .collect(),
        _ => Err("terms are [text, term] pairs".to_owned()),
    }
}

/// A group's paint: a paint's name, or `{paint?, terms?, paints?,
/// declaration?}`.
fn paint_brush(group: &str, brush: &Value) -> Result<Brush, String> {
    let paint = |value: &Value| {
        match value {
            Value::Text(paint) => paint.parse::<Paint>().ok(),
            _ => None,
        }
        .ok_or_else(|| {
            let names = <Paint as strum::VariantNames>::VARIANTS.join(", ");
            format!("'{group}' must be painted as one of {names}")
        })
    };
    let Value::Record(fields) = brush else {
        return Ok(Brush {
            paint: Some(paint(brush)?),
            ..Brush::default()
        });
    };
    if let Some(key) = unknown(fields, &["paint", "terms", "paints", "declaration"]) {
        return Err(format!("unknown paint field '{key}'"));
    }
    let paints = match fields.get("paints") {
        None => Vec::new(),
        Some(table) => term_table(table)
            .map_err(|_| format!("'{group}' paints are [term, paint] pairs"))?
            .into_iter()
            .map(|entry| Ok((entry.text, paint(&Value::Text(entry.term.to_string()))?)))
            .collect::<Result<_, String>>()?,
    };
    Ok(Brush {
        paint: fields.get("paint").map(paint).transpose()?,
        paints,
        terms: fields
            .get("terms")
            .map(|terms| strings(terms).map_err(|e| e.to_string()))
            .transpose()?
            .unwrap_or_default(),
        declaration: flag(fields.get("declaration"), false)
            .ok_or("declaration must be true or false")?,
    })
}

/// An optional Boolean field, `default` when absent, or `None` when it is
/// something else.
pub(crate) fn flag(value: Option<&Value>, default: bool) -> Option<bool> {
    match value {
        None => Some(default),
        Some(Value::Bool(value)) => Some(*value),
        Some(_) => None,
    }
}

/// A manifest record's fields.
type Fields = BTreeMap<String, Value>;

/// Each entry of the record a feature module declares as `what`, keyed by
/// `keys`, of at most [`MAX_DECLARED`] entries, read in key order.
fn entries<T>(
    declared: &Value,
    what: &str,
    keys: &str,
    read: impl Fn(&String, &Value) -> EvalResult<T>,
) -> EvalResult<Vec<T>> {
    match declared {
        Value::Record(entries) if entries.len() > MAX_DECLARED => {
            Err(format!("A module declares at most {MAX_DECLARED} {what}").into())
        }
        Value::Record(entries) => entries
            .iter()
            .map(|(key, entry)| read(key, entry))
            .collect(),
        _ => Err(format!("{what} must be a record of {keys}").into()),
    }
}

/// One entry of a declared record, `at` naming it in its problems: each
/// field is read by its shape, and a field that has another is a problem
/// `{at}.{field} must ...`.
pub(crate) struct Entry<'a> {
    at: String,
    fields: &'a Fields,
}
impl<'a> Entry<'a> {
    /// The record `fields` at `at`, whatever fields it has.
    pub(crate) fn of(at: &str, fields: &'a Fields) -> Self {
        let at = at.to_owned();
        Self { at, fields }
    }
    /// The entry at `at`: a record of `known` fields only.
    fn new(at: String, entry: &'a Value, known: &[&str]) -> EvalResult<Self> {
        let Value::Record(fields) = entry else {
            return Err(format!("{at} must be a record").into());
        };
        match unknown(fields, known) {
            Some(field) => Err(format!("{at} has no field '{field}'").into()),
            None => Ok(Self { at, fields }),
        }
    }
    fn get(&self, field: &str) -> Option<&'a Value> {
        self.fields.get(field)
    }
    /// The problem of a `field` that does not do what it `should`.
    pub(crate) fn must(&self, field: &str, should: &str) -> EvalError {
        format!("{}.{field} must {should}", self.at).into()
    }
    /// An optional text field, `default` when absent.
    fn text(&self, field: &str, default: &str) -> EvalResult<String> {
        match self.get(field) {
            None => Ok(default.to_owned()),
            Some(Value::Text(text)) => Ok(text.clone()),
            Some(_) => Err(self.must(field, "be text")),
        }
    }
    /// An optional list-of-text field.
    pub(crate) fn list(&self, field: &str) -> EvalResult<Option<Vec<String>>> {
        let list = |items| strings(items).map_err(|_| self.must(field, "be a list of text"));
        self.get(field).map(list).transpose()
    }
    /// A text field naming one of `choices`, `default` when absent.
    fn one_of<T: Copy>(
        &self,
        field: &str,
        choices: &[(&str, T)],
        default: Option<T>,
    ) -> EvalResult<T> {
        match self.get(field) {
            None => default,
            Some(Value::Text(text)) => choices.iter().find(|(name, _)| name == text).map(|c| c.1),
            Some(_) => None,
        }
        .ok_or_else(|| {
            let names: Vec<_> = choices.iter().map(|(name, _)| *name).collect();
            self.must(field, &format!("be {}", names.join(" or ")))
        })
    }
}

/// The first of `fields` that is not one of `known`.
fn unknown<'a>(fields: &'a Fields, known: &[&str]) -> Option<&'a String> {
    fields.keys().find(|k| !known.contains(&k.as_str()))
}

/// A list of text, as a module's manifest fields declare them.
pub(crate) fn strings(value: &Value) -> EvalResult<Vec<String>> {
    if let Value::List(items) = value {
        items.iter().map(String::from_value).collect()
    } else {
        Err(EvalError::Expected("a list of text"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record of `fields`, as a manifest writes one.
    fn record(fields: &[(&str, Value)]) -> Value {
        Value::record(
            fields
                .iter()
                .map(|(k, v)| ((*k).into(), v.clone()))
                .collect(),
        )
    }

    /// A recognizer's structure, terms, brushes and links are checked when
    /// its module compiles.
    #[test]
    fn recognizer_fields_are_checked() {
        let text = |s: &str| Value::Text(s.into());
        let rule = |extra: &[(&str, Value)]| {
            let mut fields = vec![
                ("name", text("stop")),
                ("on", text("line")),
                ("pattern", text("^(?<t>\\d+) (?<k>\\w+)")),
            ];
            fields.extend(extra.iter().cloned());
            record(&fields)
        };
        let day = record(&[
            ("name", text("day")),
            ("on", text("line")),
            ("pattern", text("^# ")),
            ("until", text("heading")),
        ]);
        let check = |extra: &[(&str, Value)]| {
            rules("m", &Value::list(vec![day.clone(), rule(extra)])).map_err(|e| e.to_string())
        };
        let pairs = Value::list(vec![Value::list(vec![text("Fly"), text("Depart")])]);
        // The tokens field, painting group `k` with the brush `fields` give.
        let brush = |fields: &[(&str, Value)]| ("tokens", record(&[("k", record(fields))]));
        let ok = check(&[
            ("under", text("day")),
            ("until", text("break")),
            ("terms", record(&[("k", pairs.clone())])),
            brush(&[
                ("terms", Value::list(vec![text("k")])),
                ("paints", record(&[("Depart", text("category1"))])),
                ("paint", text("heading")),
                ("declaration", Value::Bool(true)),
            ]),
            ("links", record(&[("t", text("https://example.com/?q={}"))])),
        ])
        .unwrap();
        assert_eq!(ok[1].terms("k")[0].term.as_ref(), "Depart");
        assert_eq!(
            ok[1].tokens[0].1.term_paint("depart"),
            Some(Paint::Category1)
        );
        assert_eq!(ok[1].links[0].0, "t");
        for (extra, error) in [
            (vec![("under", text("nowhere"))], "is under 'nowhere'"),
            (
                vec![("until", text("forever"))],
                "until must be heading or break",
            ),
            (
                vec![("terms", record(&[("x", pairs.clone())]))],
                "terms names 'x'",
            ),
            (
                vec![("terms", record(&[("k", Value::list(vec![text("Fly")]))]))],
                "terms are [text, term] pairs",
            ),
            (
                vec![brush(&[("terms", Value::list(vec![text("x")]))])],
                "paints 'x'",
            ),
            (
                vec![brush(&[("paints", record(&[("Depart", text("red"))]))])],
                "'k' must be painted as one of",
            ),
            (
                vec![("links", record(&[("t", text("https://example.com"))]))],
                "a link is a URL",
            ),
            (vec![("unless", text("("))], "Invalid pattern"),
        ] {
            let message = check(&extra).unwrap_err();
            assert!(message.contains(error), "{message}");
        }
        let prose = record(&[
            ("name", text("p")),
            ("on", text("prose")),
            ("pattern", text("x")),
            ("under", text("day")),
        ]);
        let message = rules("m", &Value::list(vec![day, prose]))
            .unwrap_err()
            .to_string();
        assert!(message.contains("only a line recognizer"), "{message}");
    }

    /// An attribute may hold a tagged record of the kinds it names, each a
    /// kind a module may choose; the language names none of them.
    #[test]
    fn tagged_attributes_name_their_kinds() {
        let declare = |value: Value| {
            let entry = Value::record(BTreeMap::from([("value".to_string(), value)]));
            attributes("m", &Value::record(BTreeMap::from([("a".into(), entry)])))
        };
        let kinds = |names: &[&str]| {
            let names = names.iter().map(|n| Value::Text((*n).into())).collect();
            Value::record(BTreeMap::from([("tagged".into(), Value::list(names))]))
        };
        let declared = declare(kinds(&["Alarm", "Lap_2"])).unwrap();
        assert_eq!(declared[0].value, syntax::AttributeValue::Tagged);
        assert_eq!(declared[0].kinds, ["Alarm", "Lap_2"]);
        for bad in [
            kinds(&[]),
            kinds(&["alarm"]),
            kinds(&["Number"]),
            Value::Text("tagged".into()),
        ] {
            let error = declare(bad).unwrap_err().to_string();
            assert!(error.contains("{tagged: [kinds]}"), "{error}");
        }
    }
    /// A declared collection takes a free name and says only whether it
    /// joins `entries`.
    #[test]
    fn collection_declarations_are_checked() {
        let empty = record(&[]);
        let entries = record(&[("entries", Value::Bool(true))]);
        let leaves = record(&[("entries", Value::Text("leaf".into()))]);
        let ok = collections(&record(&[
            ("days", empty.clone()),
            ("stops", entries),
            ("tasks", leaves),
        ]))
        .unwrap();
        let declared = |name: &str, entries| Declared {
            name: name.into(),
            entries,
            from: BTreeMap::new(),
        };
        assert_eq!(
            ok,
            vec![
                declared("days", Joins::None),
                declared("stops", Joins::All),
                declared("tasks", Joins::Where("leaf".into())),
            ]
        );
        for (declared, error) in [
            (
                record(&[("links", empty.clone())]),
                "not a free collection name",
            ),
            (
                record(&[("Days", empty.clone())]),
                "not a free collection name",
            ),
            (record(&[("days", Value::Bool(true))]), "must be a record"),
            (
                record(&[("days", record(&[("sorted", Value::Bool(true))]))]),
                "has no field 'sorted'",
            ),
            (
                record(&[("days", record(&[("entries", Value::Null)]))]),
                "entries must be true, false or a field name",
            ),
            (Value::list(vec![]), "must be a record of collection names"),
        ] {
            let message = collections(&declared).unwrap_err().to_string();
            assert!(message.contains(error), "{message}");
        }
    }

    #[test]
    fn form_declarations_are_checked() {
        let text = |s: &str| Value::Text(s.into());
        let list = |items: &[&str]| Value::list(items.iter().map(|s| text(s)).collect());
        let column =
            |name: &str, reads: &str| record(&[("name", text(name)), ("reads", text(reads))]);
        let plan = |table: Value| {
            record(&[
                ("params", list(&["objective: linear expression"])),
                ("reads", list(&["linear"])),
                ("table", table),
                ("unknowns", text("free")),
                (
                    "unknown",
                    record(&[("name", text("decision variable")), ("doc", text("chosen"))]),
                ),
                ("noun", text("plan")),
            ])
        };
        let rows = Value::list(vec![
            column("constraint", "name"),
            column("expression", "constraint"),
        ]);
        let declared = forms("plans", &record(&[("maximize", plan(rows.clone()))])).unwrap();
        let [form] = declared.as_slice() else {
            panic!("one form")
        };
        assert_eq!(form.name, "maximize");
        assert_eq!(form.module, "plans");
        assert_eq!(form.reads, vec![Reading::Linear]);
        assert_eq!(form.unknowns, Unknowns::Free);
        assert_eq!(form.header(), "| constraint | expression |");
        assert_eq!(
            (form.unknown.as_str(), form.noun.as_str()),
            ("decision variable", "plan")
        );
        // A form without a table: its params, how they are read, its unknowns.
        let bare = |params: &[&str], reads: &str, unknowns: &str| {
            record(&[
                ("params", list(params)),
                ("reads", list(&[reads])),
                ("unknowns", text(unknowns)),
            ])
        };
        let seek = bare(&["constraint"], "constraint", "own");
        let own = &forms("plans", &record(&[("solve", seek.clone())])).unwrap()[0];
        assert!(own.table.is_empty() && own.noun == "solve");
        for (declared, error) in [
            (record(&[("sum", seek.clone())]), "no built-in has"),
            (record(&[("solve", Value::Bool(true))]), "must be a record"),
            (
                record(&[("solve", bare(&["a", "b"], "linear", "own"))]),
                "how each of its params is read",
            ),
            (
                record(&[("solve", bare(&["a"], "name", "own"))]),
                "must be linear or constraint",
            ),
            (
                record(&[("solve", bare(&["a"], "linear", "some"))]),
                "unknowns must be free or own",
            ),
            (
                record(&[(
                    "maximize",
                    plan(Value::list(vec![column("a", "name"), column("b", "name")])),
                )]),
                "names its rows in one column at most",
            ),
            (
                record(&[("maximize", plan(Value::list(vec![column("a", "text")])))]),
                "must be linear, constraint or name",
            ),
            (Value::list(vec![]), "must be a record of form names"),
        ] {
            let message = forms("plans", &declared).unwrap_err().to_string();
            assert!(message.contains(error), "{message}");
        }
    }
}
