//! Hot-reloadable XMD modules: what one is, how it compiles, and how the
//! engine calls into it.
use chrono::{DateTime, FixedOffset};
use model::recognized::{Brush, On, Paint, Rule, Term};
use model::{Declaration, Document};
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
    path::{Path, PathBuf},
    sync::Arc,
};
use syntax::{Expr, Lexeme, Parser, lex};
use url::Url;
use values::{EvalError, EvalResult, FromValue, Value};

use crate::registry::ModuleRegistry;
use values::Collection;

/// The clock a module call runs at when native code has no clock to give
/// it: the call is handed every date it needs as an argument instead. Module
/// code evaluated at it answers `now()` and `today()` with an error rather
/// than with 1970.
pub fn no_clock() -> DateTime<FixedOffset> {
    DateTime::UNIX_EPOCH.fixed_offset()
}
/// Whether `now` is a real clock rather than [`no_clock`].
pub fn has_clock(now: DateTime<FixedOffset>) -> bool {
    now != no_clock()
}

/// What a compiled module evaluates against: its own note and the libraries it
/// imports, as the evaluator holds them. The evaluator implements it, so this
/// vocabulary describes and validates modules without naming the engine.
pub trait ModuleEnvironment: Any + Send + Sync + Debug {
    /// The note at `path`, which for a module is its own source.
    fn document(&self, path: &Path) -> &Document;
    /// Whether `name` resolves to exactly one symbol of the note at `path`.
    fn resolves(&self, path: &Path, name: &str) -> bool;
    /// The libraries the module imports, linked.
    fn modules(&self) -> &ModuleRegistry;
    /// The same notes over another set of linked libraries.
    fn with_modules(&self, modules: ModuleRegistry) -> Arc<dyn ModuleEnvironment>;
    /// One evaluator of the note at `path`'s definitions by name. A module's
    /// manifest checks share one, so they share its budget and memo.
    fn evaluator<'s>(&'s self, path: &'s Path) -> Box<Evaluator<'s>>;
    /// Call `module`'s function `name` in this environment at `now`.
    fn call(
        self: Arc<Self>,
        module: &Module,
        name: &str,
        args: Vec<Value>,
        now: DateTime<FixedOffset>,
    ) -> EvalResult<Value>;
}

/// Evaluates a definition of one note by name.
pub type Evaluator<'s> = dyn FnMut(&str) -> EvalResult<Value> + 's;

/// Builds the environment a module compiled from the note at `path` evaluates
/// against: that note alone, importing nothing until the registry links it.
pub type NewEnvironment = fn(&Path, Document) -> Arc<dyn ModuleEnvironment>;

/// What a module plugs into. `module.kind` in the source names one of these.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    strum::EnumString,
    strum::Display,
    strum::VariantArray,
    strum::IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum ModuleKind {
    /// Decorates matching URLs: inlays, hovers, properties and refreshes.
    Link,
    /// Drives editor features over the document catalog.
    Feature,
    /// Plain functions other modules and the engine import by name.
    Library,
    /// A command a person runs with `xmd run`: its pure `step` hook asks the
    /// host for effects (HTTP, files in the run's directory) and reads their
    /// results on the next step. Nothing runs while editing or rendering.
    Command,
    /// Fetches external data a note's lookups read, such as exchange rates or
    /// forecasts: the same `step` loop as a command, limited to HTTP, run only
    /// on an explicit refresh. `provides` names the lookups it answers.
    Provider,
}
impl ModuleKind {
    /// The one hook a module of this kind must supply.
    pub(crate) fn required_hook(self) -> Option<Hook> {
        match self {
            Self::Link => Some(Hook::Inlay),
            Self::Feature => Some(Hook::Collect),
            Self::Library => None,
            Self::Command | Self::Provider => Some(Hook::Step),
        }
    }
}

/// The fixed entry points a link or feature module may define. Library exports
/// are user-chosen names and stay text; these are the contract the hosts call.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    strum::AsRefStr,
    strum::Display,
    strum::VariantArray,
)]
#[strum(serialize_all = "snake_case")]
pub enum Hook {
    Collect,
    Inlay,
    Hover,
    Property,
    Refresh,
    Decode,
    Matches,
    PropertyNames,
    TimeDependent,
    Actions,
    Reduce,
    Hovers,
    Diagnostics,
    Format,
    Records,
    Symbols,
    Completions,
    Step,
}
impl Hook {
    pub(crate) fn arity(self) -> usize {
        match self {
            Self::Property | Self::Reduce | Self::Decode => 2,
            _ => 1,
        }
    }
    /// Whether some kind of module must supply this hook; such hooks are
    /// validated only as the required one.
    pub(crate) fn required(self) -> bool {
        <ModuleKind as strum::VariantArray>::VARIANTS
            .iter()
            .any(|kind| kind.required_hook() == Some(self))
    }
    /// What the host hands this hook and what it must return.
    pub fn contract(self) -> &'static HookContract {
        HOOKS
            .iter()
            .find(|contract| contract.hook == self)
            .expect("every hook is declared in HOOKS")
    }
}

/// One hook as the host calls it: which kinds of module it belongs to, what
/// goes in, and what has to come back. The other half of the stdlib contract:
/// there native code calls the library, here it calls a module's hooks.
#[derive(Clone, Copy, Debug)]
pub struct HookContract {
    pub hook: Hook,
    /// The kinds of module the host calls it on.
    pub kinds: &'static [ModuleKind],
    /// Each parameter as `name: shape`, one per [`Hook::arity`]. A lowercase
    /// shape names a record in [`HOOK_RECORDS`] or a side of a [`StepProtocol`].
    pub params: &'static [&'static str],
    pub returns: &'static str,
    pub doc: &'static str,
}

/// A record the host builds for more than one hook, declared once.
#[derive(Clone, Copy, Debug)]
pub struct HookRecord {
    pub name: &'static str,
    pub doc: &'static str,
    /// Each field as `(name, shape and meaning)`; a `?` after the name marks
    /// one that may be absent.
    pub fields: &'static [(&'static str, &'static str)],
}

/// The records hooks are handed and the actions they return, by name.
pub static HOOK_RECORDS: &[HookRecord] = &[
    HookRecord {
        name: "url",
        doc: "A link's address, split. Only http and https links reach a link module.",
        fields: &[
            ("raw", "Text: the whole URL"),
            ("host", "Text"),
            ("path", "Text"),
            ("scheme", "Text: `http` or `https`"),
        ],
    },
    HookRecord {
        name: "link context",
        doc: "What a link module sees of one link it matched.",
        fields: &[
            ("url", "url"),
            (
                "native",
                "Boolean: false in the browser, where a refresh cannot run a program",
            ),
            (
                "cached",
                "what `decode` last returned for this link, or null before any refresh. \
                 A cache entry older than link modules gives `title`, `state`, `merged`, \
                 `checks`, `review` and `fetched_at` instead",
            ),
            ("fetched_at", "DateTime of the cached data, or null"),
        ],
    },
    HookRecord {
        name: "feature context",
        doc: "What a feature module sees of the note a request is about. `range` and \
              `capabilities` are absent, not null, when a hook has no use for them.",
        fields: &[
            ("today", "Date: the request's day"),
            (
                "midnight",
                "DateTime: the start of `today` at the request's offset, the \
                 reference `at_time` places a day's times with",
            ),
            (
                "document",
                "Record: `path`, `uri` and `text` as text, `lines` as a list of text, and \
                 one list per collection the module's `inputs` names, under the \
                 collection's name. `inputs` defaults to sections, tasks, values and \
                 links (tasks, the bundled `tasks` module's, is empty while no active \
                 module declares it); `inputs: {tasks: [\"text\", \"line\"]}` keeps \
                 only those fields. \
                 With `values`, the same list is also `definitions`, each record with \
                 its first error as `error`. `recognized` holds only the module's own \
                 recognizers' matches, and like any collection is there when `inputs` \
                 names it. A collection a module declares in `collections` is named \
                 like any other, by any module",
            ),
            (
                "module",
                "Record: `id` and `revision` of the module being called",
            ),
            (
                "range?",
                "the LSP range being drawn, `{start, end}` of `{line, character}`",
            ),
            (
                "capabilities?",
                "Record: `refresh` and `views`, booleans for whether the host can \
                 refresh data and show views",
            ),
            (
                "position?",
                "the LSP position being completed or hovered, or where a `|` was just \
                 typed when formatting, `{line, character}`",
            ),
        ],
    },
    HookRecord {
        name: "collection",
        doc: "One entry of a feature module's `collections` record, under the \
              collection's name: a collection the module builds in its `records` \
              hook, which queries bind and any module's `inputs` may name like the \
              language's own. The name is an identifier no native collection and no \
              other active module has. A module declares at most 16.",
        fields: &[
            (
                "entries?",
                "Boolean: whether its records join `entries`, the collection a query \
                 lays on a timeline, beside tasks, events and stops; or Text, the name \
                 of a Boolean field, so only the records where it is true join (the \
                 `tasks` module's leaf tasks). False when absent",
            ),
            (
                "from?",
                "the collections only the `records` hook reads to build it, named \
                 like `inputs` (a list, or a record of the fields each keeps): the \
                 module's other hooks are not handed them, so a hook that runs on \
                 every edit reads the small built collection instead. Where `inputs` \
                 names one too, `records` reads it as `from` does. The `timers` \
                 module builds its timers from `values` and `mentions`",
            ),
        ],
    },
    HookRecord {
        name: "built record",
        doc: "One record `records` returns. The host keeps it as it is and fills \
              what it leaves out, and a query or module reads it like a native \
              record. A collection holds at most 4096.",
        fields: &[
            ("line", "Number: the zero-based line it is about. Required"),
            ("kind?", "Text: the collection's name when absent"),
            ("title?", "Text: the line's text when absent"),
            (
                "source?",
                "Record: `path`, `uri`, `line` (one-based) and `range`, the whole line's \
                 when absent",
            ),
            ("anchor?", "the LSP position at the line's end when absent"),
            ("errors?", "List of Text: empty when absent"),
            (
                "…",
                "anything else. A record anywhere inside with a `lookup` field, \
                 `{kind, key, label?}` as `cached(kind, key, label)` takes them (the \
                 prelude's `forecast_lookup(place, date)` builds a day's forecast), \
                 asks for a cached lookup: the host puts in its place its other fields \
                 with `display` (the value as the prelude's `lookup_display` words it, \
                 or why it cannot be read), `source` and `fetched_at`, or null when \
                 nothing is cached. A refresh fetches every lookup asked for, and the \
                 record's line offers one",
            ),
        ],
    },
    HookRecord {
        name: "recognizer",
        doc: "One entry of a feature module's `recognizes` list: a pattern the host runs \
              over every line of one kind of block whenever a note is parsed, with no \
              module code evaluated. Each non-empty match becomes a `recognized` \
              record, unless the rule only paints. Patterns, groups and paints are \
              checked when the module compiles: a bad one is a module error. A module \
              declares at most 16. The bundled `itinerary` module declares the \
              itinerary this way, and `tasks` how a task's checkbox is painted.",
        fields: &[
            (
                "name",
                "Text: an identifier, once per module; each match's `recognizer`",
            ),
            (
                "on",
                "`prose`, `item`, `heading`, `row` or `line`: which lines it reads, from \
                 where their text starts (past a heading's `#`s, past a list marker and \
                 any checkbox, at a row's first `|`, past prose's indentation; a `line` \
                 rule reads any of those lines whole). `^` anchors there. Fences and \
                 comments are never read. A `prose`, `item`, `heading` or `row` rule finds \
                 every match on a line; of one module's `line` rules, the first that \
                 matches claims the line, once",
            ),
            (
                "pattern",
                "Text: a regular expression, at most 4096 bytes, with named groups \
                 `(?<name>...)`. Matching is linear in the line",
            ),
            (
                "unless?",
                "Text: a pattern; a line it matches is not this rule's. It says what a \
                 lookahead would, which patterns do not have",
            ),
            (
                "under?",
                "Text, `line` rules only: the name of another `line` rule with `until`. A \
                 line is this rule's only while a match of that rule is open, and its \
                 match belongs to the nearest one: its record's `parent`",
            ),
            (
                "until?",
                "`heading` or `break`, `line` rules only: how long a match stays open to \
                 the matches under it. `heading`: until the next heading. `break`: until \
                 the first line that is blank or that none of them claims. A match also \
                 closes when another match claims a line under the same parent, or above",
            ),
            (
                "terms?",
                "Record: a named group's terms, `[[text, term], ...]` in order (or a \
                 record of text to term), all text. A group whose captured text is one \
                 of them, ignoring case, has that `term`",
            ),
            (
                "tokens?",
                "Record: a named group's paint, one of `keyword`, `number`, `string`, \
                 `variable`, `heading`, `function`, `property`, `decorator`, `operator`, \
                 `comment`, `punctuation`, `money`, `date`, `time`, `duration`, \
                 `boolean`, `link`, `code`, `key` (the key of a `Key: value` line), \
                 `toggle`, `toggle_on` and `toggle_mixed` (the state of a control a line \
                 carries, off, on and mixed, as a tri-state checkbox shows it: a host \
                 may make it clickable, running the line's row action), `finished` (the \
                 text of something done, struck through) or `category1` to `category10` \
                 (a categorical palette: a module that needs distinguishable hues picks \
                 categories, and the theme colors them); or \
                 `{paint?, terms?, paints?, declaration?}`: the paint of the term of the \
                 first of `terms` (group names) that has one, else `paint`, marked as a \
                 declaration when `declaration` is true. `paints` gives terms their \
                 paints, `[[term, paint], ...]` (or a record of term to paint); a term it \
                 does not list paints as the paint it names, ignoring case. The note's own \
                 structure (links, names, attributes, comments) paints over it",
            ),
            (
                "links?",
                "Record: a named group's link, a URL whose `{}` is the captured text, \
                 form-encoded. Each becomes one of the note's links",
            ),
            (
                "title?",
                "Boolean: whether it reads a line only up to where its title ends, at \
                 its first attribute or a heading's or checklist item's trailing \
                 `:name`, so those paint as themselves. False when absent",
            ),
            (
                "record?",
                "Boolean: false when its matches only paint, and are no `recognized` \
                 records (a rule that only paints has no `under` or `until`). True \
                 when absent",
            ),
        ],
    },
    HookRecord {
        name: "recognized",
        doc: "One match of a recognizer: a record of the `recognized` collection, which \
              queries read for every module and a feature module that names it in \
              `inputs` reads as `ctx.document.recognized`, for its own. Built when the note is parsed \
              and kept with its other records. A note keeps at most 4096 matches.",
        fields: &[
            ("kind", "`recognized`"),
            ("recognizer", "Text: the recognizer's `name`"),
            ("module", "Text: the id of the module that declared it"),
            ("title", "Text: the matched text"),
            ("text", "Text: the matched text"),
            ("line", "Number: the zero-based line"),
            ("range", "the match's LSP range"),
            (
                "anchor",
                "the LSP position just past the match, where an inlay goes",
            ),
            (
                "groups",
                "Record: each named group that took part, as `{text, range}`, with \
                 `term` (Text or Null) when the recognizer declares terms for it",
            ),
            (
                "parent",
                "Number or Null: the line of the match it is `under`",
            ),
            (
                "end_line",
                "Number: one past the last line it holds, the lines under it included",
            ),
            (
                "source",
                "Record: `path`, `uri`, `line` (one-based) and `range`",
            ),
            ("errors", "List: empty"),
        ],
    },
    HookRecord {
        name: "attribute",
        doc: "One entry of a feature module's `attributes` record, under the key a note \
              writes after `@`: an attribute the module owns, which no other active \
              module has. The language owns none. Notes are parsed knowing it, so its \
              value is painted, checked and completed as what it holds, and the host \
              evaluates it in the note's scope, wherever it is live, into the \
              `attributed` collection: the module reads values, never note code. A \
              value that fails is an `attribute` error on it (a `dependency` error for \
              dependencies, with the items a cycle walks). A module declares at most \
              16. The bundled `tasks` module declares a task's attributes this way, \
              and `appointments` an appointment's `@at`.",
        fields: &[
            (
                "value",
                "`when` (a date or time: relative text such as `tomorrow`, or an \
                 expression that evaluates to one), `date` (a calendar date written \
                 `YYYY-MM-DD`, never evaluated, as an editor action stamps it), \
                 `duration` (an expression that evaluates to a nonnegative duration), \
                 `dependencies` (comma-separated conditions, each a Boolean or a \
                 checklist: the value is the ones not met yet, as `{text, name, \
                 source}`; a condition that names a checklist item brings that item's \
                 own dependencies in, and a cycle is an error), `expression` (any \
                 value), `text` (never evaluated), or `{tagged: [kinds]}` (the bare \
                 name of a definition whose own call makes a tagged record of one of \
                 those kinds, which claims it as its `origin`; the value is the \
                 record)",
            ),
            (
                "params?",
                "List of Text: its parameters, as signature help shows them",
            ),
            (
                "applies?",
                "Text: which lines take it, as signature help words it",
            ),
            (
                "doc?",
                "Text: what it means, for signature help, completion and the reference",
            ),
            (
                "example?",
                "Text: the value signature help and completion fill in",
            ),
            (
                "values?",
                "List of Text: the values completion offers inside it",
            ),
            (
                "on?",
                "`checkbox` or `any`: `checkbox` makes it live only on a checklist item \
                 (a list item with a checkbox), and prose anywhere else. `any` when \
                 absent",
            ),
        ],
    },
    HookRecord {
        name: "checkbox",
        doc: "A list item with a checkbox, a checklist item, as the language reads \
              it: a record of the `checkboxes` collection, which queries read and a \
              feature module that names it in `inputs` reads as \
              `ctx.document.checkboxes`. A named item is a Boolean, whether it is \
              done, and a named heading the checklist of the items under it; what \
              else an item is, a task, the bundled `tasks` module builds from these \
              and `attributed`.",
        fields: &[
            ("kind", "`checkbox`"),
            ("line", "Number: the zero-based line"),
            (
                "title",
                "Text: its text past the checkbox, up to its first attribute or \
                 trailing `:name`, trimmed",
            ),
            ("name", "Text or Null: its trailing `:name`"),
            ("name_range", "the LSP range of its name, or null"),
            (
                "mark",
                "`open`, `in_progress` or `done`: what its checkbox says",
            ),
            (
                "done",
                "Boolean: what its name evaluates to: checked, or every subitem done \
                 when it has some",
            ),
            (
                "parent",
                "Number or Null: the line of the item it nests under, the nearest \
                 open one indented less, until a heading",
            ),
            (
                "children",
                "List of Number: the lines of the items nested under it",
            ),
            ("indent", "Number: its indentation in bytes"),
            ("checkbox", "the LSP range of its `[ ]`"),
            (
                "range",
                "the LSP range of the line's text, the blanks around it aside",
            ),
            (
                "attributes",
                "Record: each attribute it writes, as written, by key",
            ),
            ("anchor", "the LSP position at the line's end"),
            (
                "source",
                "Record: `path`, `uri`, `line` (one-based) and `range`",
            ),
            ("errors", "List: empty"),
        ],
    },
    HookRecord {
        name: "attributed",
        doc: "A line that writes an attribute a module declares live there, tasks \
              included: a record of the `attributed` collection, which queries read \
              and a feature module that names it in `inputs` reads as \
              `ctx.document.attributed`. Built with the note's other records, for \
              the request's day.",
        fields: &[
            ("kind", "`attributed`"),
            ("line", "Number: the zero-based line"),
            (
                "title",
                "Text: the line's text from where it starts (past a list marker and \
                 any checkbox) up to its first attribute, trimmed",
            ),
            ("block", "`item`, `prose` or `row`"),
            ("task", "Boolean: whether the line is a task"),
            (
                "range",
                "the LSP range of the line's text, the blanks around it aside",
            ),
            (
                "attributes",
                "Record: each declared attribute the line writes, live there, by key \
                 (the last of a repeated key), as `{text, value, date, error, range, \
                 value_range}`: `text` as written, `value` evaluated as the \
                 declaration says (null when it fails), `date` its calendar day at \
                 the request's offset when it is a date or time, `error` why it \
                 failed or null, and the LSP ranges of the whole `@key(value)` and \
                 of the value",
            ),
            ("anchor", "the LSP position at the line's end"),
            (
                "source",
                "Record: `path`, `uri`, `line` (one-based) and `range`",
            ),
            ("errors", "List: empty"),
        ],
    },
    HookRecord {
        name: "action",
        doc: "What a control does, a record with a `kind`. A `document` is a note's URI, \
              and `expected` is its whole text when the action was offered, or for a \
              `row`, that row's: the action is refused if it changed since.",
        fields: &[
            (
                "invoke",
                "`{document, expected, module, revision, event}`: call this module's \
                 `reduce` with `event` when the person runs it",
            ),
            (
                "edit",
                "`{document, expected, edits}`: apply LSP text edits",
            ),
            (
                "row",
                "`{document, row, expected, module, event}`: the row's own control, \
                 what clicking its checkbox does: call this module's `reduce` with \
                 `event` when the person runs it, against the note and clock of that \
                 moment. Refused only when the row no longer reads as `expected`, so \
                 an edit elsewhere leaves it standing. It leads its row's controls, \
                 and a host that prefers edits resolves it into one up front",
            ),
            (
                "open_resource",
                "`{target: {document, row, expected}, url}`: open a link",
            ),
            (
                "refresh_resource",
                "`{target: {document, row, expected}, url}`: refresh a link's data; \
                 needs the `refresh` capability",
            ),
            (
                "refresh",
                "`{document?}`: refresh lookups; needs the `refresh` capability",
            ),
            (
                "show_today",
                "`{}`: show the today view; needs the `views` capability",
            ),
        ],
    },
];

use ModuleKind::{Command, Feature, Link, Provider};

/// Every hook, in [`Hook`] order.
pub static HOOKS: &[HookContract] = &[
    HookContract {
        hook: Hook::Collect,
        kinds: &[Feature],
        params: &["ctx: feature context"],
        returns: "List of inlays: `{at: position, label: Text, tooltip?: Text}` or \
                  `{line: Number, label, tooltip?}`",
        doc: "The inline labels for the note, with `ctx.range` set. `at` is an LSP \
              position; `line` puts the label at that line's end. Every position is \
              checked against the text. A failure shows one `module error` label on the \
              first line. Required unless the module defines another feature hook: \
              `actions`, `hovers`, `diagnostics`, `format`, `records`, `symbols` or \
              `completions`.",
    },
    HookContract {
        hook: Hook::Inlay,
        kinds: &[Link],
        params: &["ctx: link context"],
        returns: "Text",
        doc: "The label shown after a link the module matched. A failure shows \
              `module error · ...` as the label. Required.",
    },
    HookContract {
        hook: Hook::Hover,
        kinds: &[Link],
        params: &["ctx: link context"],
        returns: "Text",
        doc: "Markdown added to the link's hover. A failure shows its message instead.",
    },
    HookContract {
        hook: Hook::Property,
        kinds: &[Link],
        params: &["ctx: link context", "name: Text"],
        returns: "any value",
        doc: "What `link.name` reads. Called only for a name the module declares in \
              `properties` and `property_names` allows for this URL. Required when \
              `properties` is declared.",
    },
    HookContract {
        hook: Hook::Refresh,
        kinds: &[Link],
        params: &["url: url"],
        returns: "`{program: Text, args: List of Text, title?: Text, env?: Record of \
                  Text, format?: \"json\" or \"feed\"}`",
        doc: "The program a refresh of this URL runs. It is data: only a native host \
              runs it, when a person asks for a refresh. A `./` or `../` program is \
              relative to the module's file. A reply that does not fit, or a failure, \
              means the link has no refresh. It sees a fixed clock, not the request's. \
              Defined together with `decode` or not at all.",
    },
    HookContract {
        hook: Hook::Decode,
        kinds: &[Link],
        params: &["url: url", "data: JSON value"],
        returns: "Record",
        doc: "Turns the program's output into what is cached for the link, which later \
              hooks see as `ctx.cached`. With `format: \"feed\"` the host parses the \
              RSS or Atom document into JSON first. Anything but a record fails the \
              refresh.",
    },
    HookContract {
        hook: Hook::Matches,
        kinds: &[Link],
        params: &["url: url"],
        returns: "Boolean",
        doc: "Whether the module takes a URL its `hosts` and `path_prefix` already \
              admit. Anything but true, a failure included, is no. Required when the \
              module declares no hosts. It sees a fixed clock, not the request's.",
    },
    HookContract {
        hook: Hook::PropertyNames,
        kinds: &[Link],
        params: &["url: url"],
        returns: "List of Text",
        doc: "Which declared properties this URL has. Names not in `properties` are \
              dropped, and a failure means none. Without it every declared property \
              applies. It sees a fixed clock, not the request's.",
    },
    HookContract {
        hook: Hook::TimeDependent,
        kinds: &[Link, Feature],
        params: &["ctx: link context or feature context"],
        returns: "Boolean",
        doc: "Whether the output depends on the clock, so it is redrawn as time \
              passes. A feature module gets `ctx.range`, and only true counts; for a \
              link module anything but false counts, a failure included. Without it, a \
              module is time dependent when it or a library it imports reads `now` or \
              `today`.",
    },
    HookContract {
        hook: Hook::Actions,
        kinds: &[Feature],
        params: &["ctx: feature context"],
        returns: "List of `{line: Number, title: Text, action: action, disabled?: Text}`",
        doc: "The controls the note's lines offer, called once for the whole note with \
              `ctx.capabilities` set; `line` is the zero-based line a control shows on. \
              An action the host cannot perform is dropped, and the rest are checked \
              before they show: one that fails hides the other controls on its line. A \
              control with `disabled` says why it cannot run now: it is no lens or \
              command, and a host that resolves row actions into edits shows it \
              disabled with that reason. A failure, or a line outside the note, shows \
              none.",
    },
    HookContract {
        hook: Hook::Reduce,
        kinds: &[Feature],
        params: &["ctx: feature context", "event: any value"],
        returns: "action, not `invoke` or `row`",
        doc: "The action an `invoke` or `row` control performs, decided when the \
              person runs it, from the `event` the control carried. `ctx.capabilities` \
              is set. For an `invoke`, the note and the module must still be at the \
              text and revision the control was offered with; for a `row`, only its \
              row must read as it did. A reducer that refuses with `error(reason)` \
              has the person read the reason as written.",
    },
    HookContract {
        hook: Hook::Hovers,
        kinds: &[Feature],
        params: &["ctx: feature context"],
        returns: "List of `{range, contents: Text, fallback?: Boolean}`",
        doc: "Markdown hovers over LSP ranges of the note, with `ctx.position` the \
              position hovered. The first module with one covering the cursor wins \
              over the editor's own hover; one with `fallback` is shown only where the \
              editor's own finds nothing more specific than the row. A failure shows \
              none.",
    },
    HookContract {
        hook: Hook::Diagnostics,
        kinds: &[Feature],
        params: &["ctx: feature context"],
        returns: "List of LSP diagnostics: `{range, message, severity?, code?, source?}`",
        doc: "Problems shown with the editor's own. `source` defaults to `xmd`. A \
              failure becomes one error diagnostic naming the module.",
    },
    HookContract {
        hook: Hook::Format,
        kinds: &[Feature],
        params: &["ctx: feature context"],
        returns: "List of LSP text edits: `{range, newText}`",
        doc: "Edits made when the note is formatted, every feature module's together: \
              the bundled `tables` module's lay tables out. All of them have to apply \
              together, or formatting fails. Typing a `|` in a table formats it too, with \
              `ctx.position` just after the pipe: the editor keeps only the edits on \
              that table's lines, and leaves the row being typed alone until it has \
              its closing pipe and a cell for every column.",
    },
    HookContract {
        hook: Hook::Records,
        kinds: &[Feature],
        params: &["ctx: feature context"],
        returns: "Record: a list of built records under each collection the module \
                  declares in `collections`",
        doc: "Builds the module's collections for one note, once per revision of \
              the note and its workspace and per day: queries, every module's \
              `ctx.document` and the host read what it returned. It runs without \
              the clock, so `ctx.today` and `ctx.midnight` are its dates and \
              `now()` or `today()` fails. `ctx.document` holds the module's \
              `inputs` but the collections modules build, `entries` included, \
              and what its collections are built `from`. A \
              failure leaves the collections empty and becomes one error \
              diagnostic naming the module, on the first line it recognized. \
              Required when the module declares `collections`.",
    },
    HookContract {
        hook: Hook::Symbols,
        kinds: &[Feature],
        params: &["ctx: feature context"],
        returns: "List of `{name: Text, detail?: Text, kind?: Text, line: Number, \
                  end_line?: Number, selection: range}`",
        doc: "Entries for the note's outline. One spans from `line` to its last \
              filled line before `end_line` (one past `line` when absent) and \
              nests by that span with the editor's own; `kind` is an LSP symbol \
              kind in snake case, `namespace` when absent. One on a heading's \
              line gives that heading's entry its detail and span instead. A \
              failure adds none.",
    },
    HookContract {
        hook: Hook::Completions,
        kinds: &[Feature],
        params: &["ctx: feature context"],
        returns: "Null, or a list of `{label: Text, insert?: Text, detail?: Text, \
                  kind?: Text}`",
        doc: "What to offer at `ctx.position`, replacing the word being typed \
              with `insert` (the label when absent); `kind` is an LSP completion \
              kind in snake case. The first module with a list answers, even an \
              empty one; null leaves the position to the editor. A failure is \
              null.",
    },
    HookContract {
        hook: Hook::Step,
        kinds: &[Command, Provider],
        params: &["ctx: step input"],
        returns: "step output",
        doc: "One step of the loop a command or provider runs. Required. Its input, \
              output and effects are the step protocol of the module's kind.",
    },
];

/// One effect a `step` may request, and what the next step reads back.
#[derive(Clone, Copy, Debug)]
pub struct Effect {
    /// The request's `kind`.
    pub kind: &'static str,
    /// The request record's fields.
    pub request: &'static str,
    /// The result's fields besides `ok`.
    pub answer: &'static str,
    pub doc: &'static str,
}

/// The `step` loop of one kind of module: what each step is handed, what it
/// returns, and the effects it may ask for. Every result has `ok`; a failed
/// effect answers `{ok: false, error}` instead of stopping the loop.
#[derive(Clone, Copy, Debug)]
pub struct StepProtocol {
    pub kind: ModuleKind,
    pub doc: &'static str,
    /// Each field of the step's input, as `(name, shape and meaning)`.
    pub input: &'static [(&'static str, &'static str)],
    /// Each field of the record a step returns; `?` marks an optional one.
    pub output: &'static [(&'static str, &'static str)],
    pub effects: &'static [Effect],
}

/// The `step` loops of commands and providers.
pub static STEPS: &[StepProtocol] = &[
    StepProtocol {
        kind: Command,
        doc: "`xmd run` calls `step` until it says `done`, performing the requests of \
              each step in order between calls. Paths are relative to the directory the \
              command runs in and cannot leave it. The clock stays where the run, or a \
              repeat, started. A malformed request or an unknown kind stops the run.",
        input: &[
            (
                "args",
                "Record: `flags`, from `--name value`, `--name=value` and bare `--flag` \
                 (true), with `-` in names read as `_`; `positional`, the other words \
                 as a list of text",
            ),
            ("dir", "Text: the name of the directory the command runs in"),
            (
                "state",
                "what the previous step returned as `state`; null at first",
            ),
            (
                "results",
                "List: `results[i]` answers the previous step's `requests[i]`; empty at \
                 first",
            ),
        ],
        output: &[
            (
                "state?",
                "any value, handed to the next step; null when absent",
            ),
            (
                "requests?",
                "List of effects to perform before the next step",
            ),
            ("report?", "List: each item is printed as a line"),
            (
                "done?",
                "Boolean: true ends the run once `requests` are performed",
            ),
            (
                "error?",
                "any value but null: the run stops with it as the message, after \
                 `report` is printed",
            ),
            (
                "repeat_after?",
                "seconds, a number or text, read when `done`: wait (a day at most), \
                 then start over with null state",
            ),
        ],
        effects: &[
            Effect {
                kind: "http",
                request: "`{method?, url, headers?, json?, pick?}`",
                answer: "`{status, json, text}`",
                doc: "An http or https request, GET unless `method` says PUT, POST, PATCH or \
                      DELETE. `json` is sent as the body. `json` in the answer is the parsed reply \
                      (null when it is not JSON, and then `text` has it). `pick` keeps only the \
                      named fields of each record in a JSON list reply.",
            },
            Effect {
                kind: "read",
                request: "`{path, json?}`",
                answer: "`{text}`, or `{json}` when `json` is true",
                doc: "Read a file, parsed when `json` is true.",
            },
            Effect {
                kind: "list",
                request: "`{path}`",
                answer: "`{files}`",
                doc: "The names of the files in a directory, sorted.",
            },
            Effect {
                kind: "write",
                request: "`{path, text}` or `{path, json}`",
                answer: "`{}`",
                doc: "Write a file, `json` as pretty JSON, making its directory.",
            },
            Effect {
                kind: "move",
                request: "`{from, to}`",
                answer: "`{}`",
                doc: "Rename a file, making the directory it moves into.",
            },
            Effect {
                kind: "remove",
                request: "`{path}`",
                answer: "`{}`",
                doc: "Delete a file.",
            },
            Effect {
                kind: "credential",
                request: "`{scope, set?}`",
                answer: "`{value}`",
                doc: "A secret saved for this command and scope in the user's config \
                      directory, stored first when `set` is given. `value` is null when \
                      nothing is saved.",
            },
            Effect {
                kind: "env",
                request: "`{name}`",
                answer: "`{value}`",
                doc: "An environment variable, null when unset or empty. Only `XMD_*` \
                      names; any other stops the run.",
            },
            Effect {
                kind: "uuid",
                request: "`{}`",
                answer: "`{value}`",
                doc: "A random identifier.",
            },
        ],
    },
    StepProtocol {
        kind: Provider,
        doc: "A refresh calls `step` for each lookup the notes want that no command in \
              `.xmd/providers.json` answers, using the first provider that `provides` \
              its kind. The loop is the command's, with a lookup instead of arguments \
              and a value instead of a report.",
        input: &[
            (
                "key",
                "Record: the lookup's `kind` and the parts of its key by name, as \
                 `cached(kind, key)` asked for it: `from` and `to` for a rate, \
                 `symbol` for a quote, `place` and `date` (a Date) for a forecast",
            ),
            ("today", "Date: the refresh's day"),
            (
                "state",
                "what the previous step returned as `state`; null at first",
            ),
            (
                "results",
                "List: `results[i]` answers the previous step's `requests[i]`; empty at \
                 first",
            ),
        ],
        output: &[
            (
                "state?",
                "any value, handed to the next step; null when absent",
            ),
            (
                "requests?",
                "List of effects to perform before the next step; only `http`",
            ),
            (
                "done?",
                "Boolean: true ends the loop, without performing `requests`",
            ),
            (
                "value?",
                "read when `done`: the lookup's value as JSON, every number stored as a \
                 decimal",
            ),
            (
                "source?",
                "Text: where the value came from; the module id by default",
            ),
            (
                "error?",
                "any value but null: this lookup fails with it as the message",
            ),
        ],
        effects: &[Effect {
            kind: "http",
            request: "`{url}`",
            answer: "`{json}`",
            doc: "A GET request. A reply that is not JSON is a failed request. Any \
                  other kind of request fails the lookup.",
        }],
    },
];

/// A collection a feature module declares and builds: the other half of
/// [`Collection::Declared`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declared {
    pub name: Arc<str>,
    /// Which of its records join `entries`.
    pub entries: Joins,
    /// The collections only the `records` hook reads to build it, each with
    /// the fields it keeps, or all of them.
    pub from: BTreeMap<Collection, Option<Vec<String>>>,
}
/// Which records of a declared collection join `entries`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Joins {
    None,
    All,
    /// Those whose field of this name is true.
    Where(Arc<str>),
}
impl Declared {
    /// The collection a query or `inputs` binds it as.
    pub fn collection(&self) -> Collection {
        Collection::Declared(self.name.clone())
    }
}

#[derive(Clone, Debug)]
pub struct Module {
    pub id: String,
    pub kind: ModuleKind,
    pub path: PathBuf,
    pub live: bool,
    pub(crate) own_live: bool,
    pub enabled: bool,
    pub inputs: Vec<Collection>,
    pub fields: BTreeMap<Collection, Vec<String>>,
    /// What the `records` hook reads besides `inputs`: every collection a
    /// declared collection is built `from`, with the fields it keeps, or
    /// all of them. The module's other hooks are not handed these.
    pub sources: BTreeMap<Collection, Option<Vec<String>>>,
    /// The recognizers a feature module declares in `recognizes`, compiled.
    pub recognizes: Vec<Arc<Rule>>,
    /// The collections a feature module declares in `collections` and builds
    /// in its `records` hook.
    pub collections: Vec<Declared>,
    /// The attributes a feature module declares in `attributes`: notes are
    /// parsed knowing them, and the host evaluates them.
    pub attributes: Vec<Arc<Declaration>>,
    pub(crate) imports: Vec<String>,
    /// The lookup kinds a provider module answers (`rate`, `quote`, `forecast`).
    pub provides: Vec<String>,
    /// The declared public API, or `None` for the older rule that every
    /// non-`_` definition is public. See [`Module::public_names`].
    pub(crate) exports: Option<Vec<String>>,
    /// What a library declares in `accepts`: for a function it defines, the
    /// kind of value each argument takes, by name, which completion offers
    /// inside a call to it. Nothing checks a call against it.
    pub accepts: BTreeMap<String, Vec<String>>,
    pub(crate) expressions: Arc<BTreeMap<String, Expr>>,
    pub(crate) hosts: Vec<String>,
    pub(crate) prefix: String,
    pub(crate) properties: Vec<String>,
    pub(crate) cache_key: Option<String>,
    /// Replaced only through [`Module::set_environment`], which forgets the
    /// revision.
    pub(crate) environment: Arc<dyn ModuleEnvironment>,
    /// [`Module::revision`], once asked: it reads only the environment.
    revision: std::sync::OnceLock<String>,
}
impl Module {
    /// What the module's closures evaluate against.
    pub fn environment(&self) -> &Arc<dyn ModuleEnvironment> {
        &self.environment
    }
    /// The module's definitions, which its closures resolve names in.
    pub fn expressions(&self) -> &Arc<BTreeMap<String, Expr>> {
        &self.expressions
    }
    /// Evaluate against `environment` from now on.
    pub(crate) fn set_environment(&mut self, environment: Arc<dyn ModuleEnvironment>) {
        self.environment = environment;
        self.revision = std::sync::OnceLock::new();
    }
    /// A hash of the module's text and of every module it sees.
    pub fn revision(&self) -> String {
        self.revision
            .get_or_init(|| {
                use std::hash::{Hash, Hasher};
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                self.environment.document(&self.path).text.hash(&mut hash);
                for module in &self.environment.modules().modules {
                    module.revision().hash(&mut hash);
                }
                format!("{:016x}", hash.finish())
            })
            .clone()
    }
    /// The members a note sees through `import(id)`, in the order the module
    /// declares them: its `exports` list, or, when it declares none, every
    /// definition that is not `module` and not `_`-prefixed. The reference
    /// and completion describe exactly this list, so there is one answer to
    /// "what is this library's API".
    pub fn public_names(&self) -> Vec<String> {
        match &self.exports {
            Some(exports) => exports.clone(),
            None => self.member_names(),
        }
    }
    /// Every name another module's `imports:` may reach: all definitions but
    /// `module` and the `_`-prefixed ones, exported or not. Module code is
    /// trusted the way the Rust adapters are, so the internals of `timer` or
    /// `plan` stay callable from `timers`, `plans` or the prelude while
    /// `exports` keeps them out of notes.
    pub fn member_names(&self) -> Vec<String> {
        self.environment
            .document(&self.path)
            .definitions
            .iter()
            .map(|d| d.named.name.as_str())
            .filter(|name| *name != "module" && !name.starts_with('_'))
            .map(str::to_owned)
            .collect()
    }
    /// Whether `name` is one of [`Self::public_names`].
    pub fn is_public(&self, name: &str) -> bool {
        match &self.exports {
            Some(exports) => exports.iter().any(|n| n == name),
            None => self.member_names().iter().any(|n| n == name),
        }
    }
    /// Whether the module defines `entry`: a typed [`Hook`] or, for library
    /// modules whose exports are user-defined, a plain function name.
    pub fn has(&self, entry: impl AsRef<str>) -> bool {
        self.environment.resolves(&self.path, entry.as_ref())
    }
    pub fn call(
        &self,
        entry: impl AsRef<str>,
        args: Vec<Value>,
        now: DateTime<FixedOffset>,
    ) -> EvalResult<Value> {
        let name = entry.as_ref();
        if !self.enabled {
            return Err(EvalError::ModuleDisabled(self.id.clone()));
        }
        // What comes from outside, a fetched reply or cached data, is held
        // to the size any value may have. A feature module is handed the
        // note itself, as large as the note is, and what its call may build
        // grows with that, which the environment measures.
        if self.kind != ModuleKind::Feature {
            for arg in &args {
                values::check_size(arg)?;
            }
        }
        self.environment.clone().call(self, name, args, now)
    }
}

pub fn is_module_path(path: &Path) -> bool {
    common::is_note(path)
        && path.parent().is_some_and(|p| {
            p.file_name().is_some_and(|s| s == "stdlib")
                || (p.file_name().is_some_and(|s| s == "modules")
                    && p.parent()
                        .is_some_and(|p| p.file_name().is_some_and(|s| s == ".xmd")))
        })
}

impl Module {
    /// Parse a module's source and validate its contract.
    ///
    /// A module is a `.xmd` file whose `module :=` record says what it is:
    /// `{api: 1, id, kind, inputs?, imports?, hosts?, path_prefix?, properties?,
    /// enabled?, cache_version?, cache_namespace?, exports?, accepts?,
    /// recognizes?, collections?, attributes?}`.
    ///
    /// `accepts` (libraries only) is a record from a function the module
    /// defines to the kind names its arguments take, in order:
    /// `{remind: ["Duration", "DateTime"]}`. Completion inside
    /// a call offers the names, built-ins and literals of that kind; nothing
    /// else reads it.
    ///
    /// `recognizes` (feature modules only) declares patterns the host runs
    /// over a note's generic blocks as it parses them, without evaluating
    /// anything; see the `recognizer` record in [`HOOK_RECORDS`].
    ///
    /// `exports` is an optional list of text naming a library's public API.
    /// `import(id)` from a note returns exactly those members, the reference lists
    /// exactly those, and completion offers exactly those. Each name must be a
    /// top-level definition that is neither `module` nor `_`-prefixed, and may
    /// appear once. A library that declares no `exports` keeps the older rule,
    /// every non-`_` definition is public; `exports: []` is a library only the
    /// engine and other modules call. Link and feature modules have hooks, not
    /// exports, so for them the field must be absent or empty. Another module's
    /// `imports:` is not bound by `exports`: module code may reach any non-`_`
    /// name of a library it declares (see `Module::member_names`).
    pub(crate) fn compile(
        path: PathBuf,
        source: String,
        environment: NewEnvironment,
    ) -> EvalResult<Self> {
        if source.len() > 65_536 {
            return Err("Modules are limited to 64 KiB".into());
        }
        let document = Document::parse(source);
        if let Some(problem) = document.problems.first() {
            return Err(problem.message.clone().into());
        }
        let mut names = BTreeSet::new();
        let mut live = false;
        let mut expressions = BTreeMap::new();
        for def in &document.definitions {
            if !names.insert(def.named.name.clone()) {
                return Err(format!("Duplicate definition '{}'", def.named.name).into());
            }
            if !def.expression {
                return Err("Module definitions must use :=".into());
            }
            expressions.insert(
                def.source.clone(),
                Parser::parse(&def.source)
                    .map_err(|e| format!("{}:{}: {e}", path.display(), def.value_span.line + 1))?,
            );
            live |= lex(&def.source)
                .map_err(EvalError::Message)?
                .iter()
                .any(|t| matches!(&t.kind,Lexeme::Name(n) if n=="now" || n=="today"));
        }
        let environment = environment(&path, document);
        let mut named = environment.evaluator(&path);
        let Value::Record(config) = named("module")? else {
            return Err("module must be a record".into());
        };
        let opt_strings = |key| {
            config
                .get(key)
                .map(strings)
                .transpose()
                .map(Option::unwrap_or_default)
        };
        if !matches!(config.get("api"),Some(Value::Number(n)) if *n==1.0) {
            return Err("module.api must be 1".into());
        }
        let id = String::from_value(config.get("id").ok_or("module.id is required")?)?;
        if id.is_empty()
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        {
            return Err("Invalid module id".into());
        }
        let kind: ModuleKind =
            String::from_value(config.get("kind").ok_or("module.kind is required")?)?
                .parse()
                .map_err(|_| "module.kind must be link, feature, command, provider, or library")?;
        let enabled = match config.get("enabled") {
            None => true,
            Some(Value::Bool(v)) => *v,
            _ => return Err("enabled must be boolean".into()),
        };
        let mut fields = BTreeMap::new();
        let inputs: Vec<Collection> = match config.get("inputs") {
            None => default_inputs(),
            Some(Value::Record(selections)) => {
                for (name, selection) in selections.iter() {
                    fields.insert(name.parse()?, strings(selection)?);
                }
                fields.keys().cloned().collect()
            }
            Some(value) => strings(value)?
                .iter()
                .map(|input| input.parse())
                .collect::<Result<_, String>>()?,
        };
        let recognizes = match config.get("recognizes") {
            None => vec![],
            Some(declared) if kind == ModuleKind::Feature => rules(&id, declared)?,
            Some(_) => return Err("Only feature modules declare recognizes".into()),
        };
        let collections = match config.get("collections") {
            None => vec![],
            Some(declared) if kind == ModuleKind::Feature => collections(declared)?,
            Some(_) => return Err("Only feature modules declare collections".into()),
        };
        let attributes = match config.get("attributes") {
            None => vec![],
            Some(declared) if kind == ModuleKind::Feature => attributes(&id, declared)?,
            Some(_) => return Err("Only feature modules declare attributes".into()),
        };
        if enabled && collections.is_empty() == names.contains(Hook::Records.as_ref()) {
            return Err(if collections.is_empty() {
                "records builds the collections a module declares; declare them in collections"
            } else {
                "A module that declares collections builds them in a records function"
            }
            .into());
        }
        // `hosts: "*"` is the host-agnostic spelling of an empty list: the
        // module recognizes a URL shape on any site, through its own `matches`.
        let hosts = match config.get("hosts") {
            Some(Value::Text(any)) if any == "*" => vec![],
            _ => opt_strings("hosts")?,
        };
        if enabled
            && kind == ModuleKind::Link
            && hosts.iter().any(|host| {
                Url::parse(&format!("https://{host}")).is_err()
                    || host.contains(['/', '?', '#', '@', ':'])
                    || host.to_lowercase() != *host
            })
        {
            return Err("Link modules require lowercase host names".into());
        }
        // Without hosts nothing narrows the module but its own predicate, so a
        // host-agnostic link module has to supply one.
        if enabled && kind == ModuleKind::Link && hosts.is_empty() && !names.contains("matches") {
            return Err("Link modules require hosts or a matches function".into());
        }
        let prefix = config
            .get("path_prefix")
            .map(String::from_value)
            .transpose()?
            .unwrap_or_default();
        let properties = opt_strings("properties")?;
        if properties
            .iter()
            .any(|p| !model::identifier(p) || matches!(p.as_str(), "url" | "exists"))
        {
            return Err("Invalid or reserved property name".into());
        }
        // A library's exports are its own names, so only link and feature
        // modules are checked against the hook table: required hook first.
        if let Some(required) = kind.required_hook() {
            let optional = <Hook as strum::VariantArray>::VARIANTS
                .iter()
                .copied()
                .filter(|h| !h.required());
            for hook in std::iter::once(required).chain(optional) {
                let name = hook.as_ref();
                let arity = hook.arity();
                if names.contains(name) {
                    if !matches!(named(name)?,Value::Function(f) if f.params.len()==arity) {
                        return Err(
                            format!("{name} must be a function with {arity} parameters").into()
                        );
                    }
                } else if hook == required
                    && enabled
                    && (kind == ModuleKind::Link
                        || ![
                            Hook::Actions,
                            Hook::Hovers,
                            Hook::Diagnostics,
                            Hook::Format,
                            Hook::Records,
                            Hook::Symbols,
                            Hook::Completions,
                        ]
                        .iter()
                        .any(|h| names.contains(h.as_ref())))
                {
                    return Err(format!("Missing {name} function").into());
                }
            }
        }
        if names.contains(Hook::Refresh.as_ref()) != names.contains(Hook::Decode.as_ref()) {
            return Err("refresh and decode must be supplied together".into());
        }
        if !properties.is_empty() && !names.contains(Hook::Property.as_ref()) {
            return Err("Declared properties need a property function".into());
        }
        let provides = opt_strings("provides")?;
        if kind == ModuleKind::Provider && provides.is_empty() {
            return Err(
                "A provider module provides one or more kinds of lookup, such as rate, quote or forecast"
                    .into(),
            );
        }
        if kind != ModuleKind::Provider && !provides.is_empty() {
            return Err("Only provider modules declare provides".into());
        }
        let exports = config
            .get("exports")
            .map(strings)
            .transpose()
            .map_err(|_| EvalError::from("module.exports must be a list of text"))?;
        if let Some(exports) = &exports {
            if kind != ModuleKind::Library && !exports.is_empty() {
                return Err(format!(
                    "A {kind} module has no exports; its hooks are called by the host"
                )
                .into());
            }
            let mut seen = BTreeSet::new();
            for name in exports {
                if name == "module" || name.starts_with('_') {
                    return Err(format!(
                        "'{name}' cannot be exported; 'module' and '_' names are private"
                    )
                    .into());
                }
                if !names.contains(name) {
                    return Err(format!(
                        "exports names '{name}', which this module does not define"
                    )
                    .into());
                }
                if !seen.insert(name) {
                    return Err(format!("Duplicate export '{name}'").into());
                }
            }
        }
        let accepts = match config.get("accepts") {
            None => BTreeMap::new(),
            Some(_) if kind != ModuleKind::Library => {
                return Err("Only library modules declare accepts".into());
            }
            Some(Value::Record(entries)) => entries
                .iter()
                .map(|(name, kinds)| {
                    if name == "module" || name.starts_with('_') || !names.contains(name) {
                        return Err(EvalError::from(format!(
                            "accepts names '{name}', which this module does not define"
                        )));
                    }
                    let kinds = strings(kinds)
                        .ok()
                        .filter(|kinds| kinds.iter().all(|k| common::ValueType::is_name(k)))
                        .ok_or_else(|| {
                            EvalError::from(format!(
                                "accepts.{name} must be a list of kind names, such as Duration"
                            ))
                        })?;
                    Ok((name.clone(), kinds))
                })
                .collect::<EvalResult<_>>()?,
            Some(_) => return Err("module.accepts must be a record of function names".into()),
        };
        let version = match config.get("cache_version") {
            None => "1".into(),
            Some(Value::Number(n)) if *n >= 1.0 && n.fract() == 0.0 => n.to_string(),
            _ => return Err("cache_version must be a positive integer".into()),
        };
        let cache_key = match config.get("cache_namespace") {
            Some(Value::Null) => None,
            None => Some(format!("{id}:{version}")),
            _ => return Err("cache_namespace may only be null (legacy cache) or omitted".into()),
        };
        drop(named);
        Ok(Self {
            id,
            kind,
            path,
            live,
            own_live: live,
            enabled,
            inputs,
            sources: collections
                .iter()
                .flat_map(|declared| declared.from.clone())
                .collect(),
            recognizes,
            collections,
            attributes,
            imports: opt_strings("imports")?,
            provides,
            fields,
            hosts,
            prefix,
            properties,
            exports,
            accepts,
            cache_key,
            expressions: Arc::new(expressions),
            environment,
            revision: std::sync::OnceLock::new(),
        })
    }
}

/// The most recognizers one module declares.
const MAX_RULES: usize = 16;

/// What a feature module that names no `inputs` reads: sections, tasks,
/// values and links. `tasks` is the bundled `tasks` module's collection, so
/// it holds nothing while no active module declares it.
pub(crate) fn default_inputs() -> Vec<Collection> {
    vec![
        Collection::Sections,
        Collection::Declared("tasks".into()),
        Collection::Values,
        Collection::Links,
    ]
}
/// The most collections one module declares.
const MAX_COLLECTIONS: usize = 16;

/// `collections: {name: {entries?, from?}}`: each name one no native
/// collection has. Whether another module declares it too is the registry's check.
fn collections(declared: &Value) -> EvalResult<Vec<Declared>> {
    let Value::Record(entries) = declared else {
        return Err("collections must be a record of collection names".into());
    };
    if entries.len() > MAX_COLLECTIONS {
        return Err(format!("A module declares at most {MAX_COLLECTIONS} collections").into());
    }
    entries
        .iter()
        .map(|(name, entry)| {
            let Ok(Collection::Declared(name)) = name.parse::<Collection>() else {
                return Err(format!(
                    "collections cannot declare '{name}': it is not a free collection name"
                )
                .into());
            };
            let Value::Record(fields) = entry else {
                return Err(format!("collections.{name} must be a record").into());
            };
            if let Some(key) = fields
                .keys()
                .find(|k| !matches!(k.as_str(), "entries" | "from"))
            {
                return Err(format!("collections.{name} has no field '{key}'").into());
            }
            let from = match fields.get("from") {
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
            let entries = match fields.get("entries") {
                None | Some(Value::Bool(false)) => Joins::None,
                Some(Value::Bool(true)) => Joins::All,
                Some(Value::Text(field)) if model::identifier(field) => {
                    Joins::Where(field.as_str().into())
                }
                Some(_) => {
                    return Err(format!(
                        "collections.{name}.entries must be true, false or a field name"
                    )
                    .into());
                }
            };
            Ok(Declared {
                name,
                entries,
                from,
            })
        })
        .collect()
}

/// The most attributes one module declares.
const MAX_ATTRIBUTES: usize = 16;

/// `attributes: {key: {value, params?, applies?, doc?, example?, values?,
/// on?}}`.
/// Whether another module declares the key too is the registry's check.
fn attributes(module: &str, declared: &Value) -> EvalResult<Vec<Arc<Declaration>>> {
    let Value::Record(entries) = declared else {
        return Err("attributes must be a record of attribute keys".into());
    };
    if entries.len() > MAX_ATTRIBUTES {
        return Err(format!("A module declares at most {MAX_ATTRIBUTES} attributes").into());
    }
    entries
        .iter()
        .map(|(key, entry)| {
            if !model::identifier(key) {
                return Err(format!("Invalid attribute key '{key}'").into());
            }
            let Value::Record(fields) = entry else {
                return Err(format!("attributes.{key} must be a record").into());
            };
            if let Some(field) = fields.keys().find(|k| {
                !matches!(
                    k.as_str(),
                    "value" | "params" | "applies" | "doc" | "example" | "values" | "on"
                )
            }) {
                return Err(format!("attributes.{key} has no field '{field}'").into());
            }
            let text = |field: &str, default: &str| match fields.get(field) {
                None => Ok(default.to_owned()),
                Some(Value::Text(text)) => Ok(text.clone()),
                Some(_) => Err(EvalError::from(format!(
                    "attributes.{key}.{field} must be text"
                ))),
            };
            let (value, kinds) = match fields.get("value") {
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
                format!(
                    "attributes.{key}.value must be one of {}, or {{tagged: [kinds]}} \
                     naming the kinds of tagged record it takes",
                    syntax::AttributeValue::NAMES.join(", ")
                )
            })?;
            let list = |field: &str| match fields.get(field) {
                None => Ok(vec![]),
                Some(items) => strings(items).map_err(|_| {
                    EvalError::from(format!("attributes.{key}.{field} must be a list of text"))
                }),
            };
            let params = list("params")?;
            let values = list("values")?;
            let checkbox = match fields.get("on") {
                None => false,
                Some(Value::Text(on)) if on == "checkbox" => true,
                Some(Value::Text(on)) if on == "any" => false,
                Some(_) => {
                    return Err(format!("attributes.{key}.on must be checkbox or any").into());
                }
            };
            Ok(Arc::new(Declaration {
                key: key.clone(),
                module: module.into(),
                value,
                kinds,
                params,
                applies: text("applies", "attribute")?,
                documentation: text("doc", "")?,
                example: text("example", "")?,
                values,
                checkbox,
            }))
        })
        .collect()
}

/// `recognizes: [{name, on, pattern, unless?, under?, until?, terms?,
/// tokens?, links?}]`, each pattern compiled and every group a field names
/// checked against the pattern's named groups.
fn rules(module: &str, declared: &Value) -> EvalResult<Vec<Arc<Rule>>> {
    let Value::List(items) = declared else {
        return Err("recognizes must be a list of records".into());
    };
    if items.len() > MAX_RULES {
        return Err(format!("A module declares at most {MAX_RULES} recognizers").into());
    }
    let mut names = BTreeSet::new();
    let mut rules = Vec::new();
    for item in items.iter() {
        let Value::Record(fields) = item else {
            return Err("recognizes must be a list of records".into());
        };
        if let Some(key) = fields.keys().find(|k| {
            !matches!(
                k.as_str(),
                "name"
                    | "on"
                    | "pattern"
                    | "unless"
                    | "under"
                    | "until"
                    | "terms"
                    | "tokens"
                    | "links"
                    | "title"
                    | "record"
            )
        }) {
            return Err(format!("Unknown recognizer field '{key}'").into());
        }
        let text = |key: &str| match fields.get(key) {
            Some(Value::Text(text)) => Ok(text.clone()),
            _ => Err(EvalError::from(format!(
                "Each recognizer needs {key} as text"
            ))),
        };
        let name = text("name")?;
        if !model::identifier(&name) {
            return Err(format!("Invalid recognizer name '{name}'").into());
        }
        if !names.insert(name.clone()) {
            return Err(format!("Duplicate recognizer '{name}'").into());
        }
        let fail = |message: String| EvalError::from(format!("Recognizer '{name}': {message}"));
        let on: On = text("on")?
            .parse()
            .map_err(|_| fail("on must be prose, item, heading, row or line".into()))?;
        let compile = |source: &str| common::Pattern::new(source).map(Arc::new).map_err(&fail);
        let mut rule = Rule::new(module, &name, on, compile(&text("pattern")?)?);
        let optional = |key: &str| match fields.get(key) {
            None => Ok(None),
            Some(Value::Text(text)) => Ok(Some(text.clone())),
            Some(_) => Err(fail(format!("{key} must be text"))),
        };
        rule.unless = optional("unless")?.as_deref().map(compile).transpose()?;
        rule.under = optional("under")?;
        rule.until = optional("until")?
            .map(|until| {
                until
                    .parse()
                    .map_err(|_| fail("until must be heading or break".into()))
            })
            .transpose()?;
        if on != On::Line && (rule.under.is_some() || rule.until.is_some()) {
            return Err(fail("only a line recognizer has under or until".into()));
        }
        rule.title = match fields.get("title") {
            None => false,
            Some(Value::Bool(title)) => *title,
            Some(_) => return Err(fail("title must be true or false".into())),
        };
        rule.record = match fields.get("record") {
            None => true,
            Some(Value::Bool(record)) => *record,
            Some(_) => return Err(fail("record must be true or false".into())),
        };
        if !rule.record && (rule.under.is_some() || rule.until.is_some()) {
            return Err(fail(
                "a recognizer that only paints has no under or until".into(),
            ));
        }
        let group = |group: &str, field: &str| {
            if rule.pattern.group_names().any(|g| g == group) {
                Ok(group.to_owned())
            } else if field == "tokens" {
                Err(EvalError::from(format!(
                    "Recognizer '{name}' paints '{group}', which its pattern does not name"
                )))
            } else {
                Err(fail(format!(
                    "{field} names '{group}', which its pattern does not name"
                )))
            }
        };
        let record = |key: &str| match fields.get(key) {
            None => Ok(None),
            Some(Value::Record(fields)) => Ok(Some(fields.clone())),
            Some(_) => Err(fail(format!("{key} must be a record"))),
        };
        let mut terms = Vec::new();
        for (name, table) in record("terms")?.iter().flat_map(|r| r.iter()) {
            terms.push((group(name, "terms")?, term_table(table).map_err(&fail)?));
        }
        let mut tokens = Vec::new();
        for (name, brush) in record("tokens")?.iter().flat_map(|r| r.iter()) {
            let brush = paint_brush(name, brush).map_err(&fail)?;
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
                _ => return Err(fail("a link is a URL with {} for the text".into())),
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
            format!(
                "'{group}' must be painted as one of {}",
                <Paint as strum::VariantNames>::VARIANTS.join(", ")
            )
        })
    };
    let Value::Record(fields) = brush else {
        return Ok(Brush {
            paint: Some(paint(brush)?),
            ..Brush::default()
        });
    };
    if let Some(key) = fields
        .keys()
        .find(|k| !matches!(k.as_str(), "paint" | "terms" | "paints" | "declaration"))
    {
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
        declaration: match fields.get("declaration") {
            None => false,
            Some(Value::Bool(declaration)) => *declaration,
            Some(_) => return Err("declaration must be true or false".into()),
        },
    })
}

/// A list of text, as a module's manifest fields declare them.
fn strings(value: &Value) -> EvalResult<Vec<String>> {
    if let Value::List(items) = value {
        items.iter().map(String::from_value).collect()
    } else {
        Err(EvalError::Expected("a list of text"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::VariantArray;

    /// Every hook is declared once, with a parameter per argument the host
    /// passes, on the kinds of module that have hooks.
    #[test]
    fn every_hook_is_declared_once() {
        for hook in Hook::VARIANTS {
            let declared: Vec<_> = HOOKS.iter().filter(|c| c.hook == *hook).collect();
            assert_eq!(
                declared.len(),
                1,
                "{hook} is declared {} times",
                declared.len()
            );
            let contract = declared[0];
            assert_eq!(contract.params.len(), hook.arity(), "{hook}'s params");
            assert!(!contract.kinds.is_empty() && !contract.kinds.contains(&ModuleKind::Library));
        }
        assert_eq!(HOOKS.len(), Hook::VARIANTS.len());
        for kind in ModuleKind::VARIANTS {
            if let Some(hook) = kind.required_hook() {
                assert!(hook.contract().kinds.contains(kind), "{hook} on {kind}");
            }
        }
    }

    /// A recognizer's structure, terms, brushes and links are checked when
    /// its module compiles.
    #[test]
    fn recognizer_fields_are_checked() {
        let text = |s: &str| Value::Text(s.into());
        let record = |fields: &[(&str, Value)]| {
            Value::record(
                fields
                    .iter()
                    .map(|(k, v)| ((*k).into(), v.clone()))
                    .collect(),
            )
        };
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
        let ok = check(&[
            ("under", text("day")),
            ("until", text("break")),
            ("terms", record(&[("k", pairs.clone())])),
            (
                "tokens",
                record(&[(
                    "k",
                    record(&[
                        ("terms", Value::list(vec![text("k")])),
                        ("paints", record(&[("Depart", text("category1"))])),
                        ("paint", text("heading")),
                        ("declaration", Value::Bool(true)),
                    ]),
                )]),
            ),
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
                vec![(
                    "tokens",
                    record(&[("k", record(&[("terms", Value::list(vec![text("x")]))]))]),
                )],
                "paints 'x'",
            ),
            (
                vec![(
                    "tokens",
                    record(&[(
                        "k",
                        record(&[("paints", record(&[("Depart", text("red"))]))]),
                    )]),
                )],
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

    /// A declared collection takes a free name and says only whether it
    /// joins `entries`.
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
    #[test]
    fn collection_declarations_are_checked() {
        let record = |fields: &[(&str, Value)]| {
            Value::record(
                fields
                    .iter()
                    .map(|(k, v)| ((*k).into(), v.clone()))
                    .collect(),
            )
        };
        let empty = record(&[]);
        let entries = record(&[("entries", Value::Bool(true))]);
        let leaves = record(&[("entries", Value::Text("leaf".into()))]);
        let ok = collections(&record(&[
            ("days", empty.clone()),
            ("stops", entries),
            ("tasks", leaves),
        ]))
        .unwrap();
        assert_eq!(
            ok,
            vec![
                Declared {
                    name: "days".into(),
                    entries: Joins::None,
                    from: BTreeMap::new(),
                },
                Declared {
                    name: "stops".into(),
                    entries: Joins::All,
                    from: BTreeMap::new(),
                },
                Declared {
                    name: "tasks".into(),
                    entries: Joins::Where("leaf".into()),
                    from: BTreeMap::new(),
                }
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

    /// Commands and providers each have one step protocol, and nothing else does.
    #[test]
    fn every_step_kind_has_a_protocol() {
        for kind in ModuleKind::VARIANTS {
            let protocols = STEPS.iter().filter(|p| p.kind == *kind).count();
            let steps = Hook::Step.contract().kinds.contains(kind);
            assert_eq!(protocols, usize::from(steps), "{kind}");
        }
    }
}
