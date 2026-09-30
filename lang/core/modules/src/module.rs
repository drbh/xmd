//! Hot-reloadable XMD modules: what one is, how it compiles, and how the
//! engine calls into it.
use chrono::{DateTime, FixedOffset};
use model::Document;
use model::recognized::{On, Paint, Rule};
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
    path::{Path, PathBuf},
    sync::Arc,
};
use syntax::{Expr, Lexeme, Parser, lex};
use url::Url;
use values::{EvalError, EvalResult, FromValue, LookupKind, Value};

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
                "document",
                "Record: `path`, `uri` and `text` as text, `lines` as a list of text, and \
                 one list per collection the module's `inputs` names, under the \
                 collection's name. `inputs` defaults to sections, tasks, values and \
                 links; `inputs: {tasks: [\"text\", \"line\"]}` keeps only those fields. \
                 With `values`, the same list is also `definitions`, each record with \
                 its first error as `error`. A module that declares `recognizes` also \
                 has `recognized`: its own recognizers' matches",
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
        ],
    },
    HookRecord {
        name: "recognizer",
        doc: "One entry of a feature module's `recognizes` list: a pattern the host runs \
              over every line of one kind of block whenever a note is parsed, with no \
              module code evaluated. Each non-empty match becomes a `recognized` \
              record. The pattern is checked when the module compiles: a bad one is a \
              module error. A module declares at most 16.",
        fields: &[
            (
                "name",
                "Text: an identifier, once per module; each match's `recognizer`",
            ),
            (
                "on",
                "`prose`, `item`, `heading` or `row`: which lines it reads, from where \
                 their text starts (past a heading's `#`s, past a list marker and any \
                 checkbox, at a row's first `|`, past prose's indentation). `^` anchors \
                 there. Fences and comments are never read",
            ),
            (
                "pattern",
                "Text: a regular expression, at most 1024 bytes, with named groups \
                 `(?<name>...)`. Matching is linear in the line",
            ),
            (
                "tokens?",
                "Record: a named group's paint, one of `keyword`, `number`, `string`, \
                 `variable`, `heading`, `function`, `property`, `decorator`, `operator`, \
                 `comment`, `punctuation`, `money`, `date`, `time`, `duration`, \
                 `boolean`, `link`, `code` or `place`. The note's own structure (links, \
                 names, attributes, comments) paints over it",
            ),
        ],
    },
    HookRecord {
        name: "recognized",
        doc: "One match of a recognizer: a record of the `recognized` collection, which \
              queries read for every module and a feature module reads as \
              `ctx.document.recognized` for its own. Built when the note is parsed \
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
                "Record: each named group that took part, as `{text, range}`",
            ),
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
              and `expected` is its whole text when the action was offered: the action \
              is refused if the note changed since.",
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
                "toggle_task",
                "`{document, row, expected}`: check or uncheck a task",
            ),
            (
                "timer",
                "`{document, name, action}`: `start`, `pause`, `resume` or `reset` a timer",
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
              first line. Required unless the module defines `actions`, `hovers`, \
              `diagnostics` or `format`.",
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
        returns: "List of `{line: Number, title: Text, action: action}`",
        doc: "The controls the note's lines offer, called once for the whole note with \
              `ctx.capabilities` set; `line` is the zero-based line a control shows on. \
              An action the host cannot perform is dropped, and the rest are checked \
              before they show: one that fails hides the other controls on its line. A \
              failure, or a line outside the note, shows none.",
    },
    HookContract {
        hook: Hook::Reduce,
        kinds: &[Feature],
        params: &["ctx: feature context", "event: any value"],
        returns: "action, not `invoke`",
        doc: "The action an `invoke` control performs, decided when the person runs \
              it, from the `event` the control carried. `ctx.capabilities` is set. The \
              note and the module must still be at the text and revision the control \
              was offered with.",
    },
    HookContract {
        hook: Hook::Hovers,
        kinds: &[Feature],
        params: &["ctx: feature context"],
        returns: "List of `{range, contents: Text}`",
        doc: "Markdown hovers over LSP ranges of the note. The first module with one \
              covering the cursor wins over the editor's own hover. A failure shows \
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
        doc: "Edits made when the note is formatted, after table formatting. All of \
              them have to apply together, or formatting fails.",
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
                "Record: `kind` (`rate`, `quote` or `forecast`) and the lookup's fields \
                 as text: `from` and `to`, `symbol`, or `place` and `date` (a Date)",
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
    /// The recognizers a feature module declares in `recognizes`, compiled.
    pub recognizes: Vec<Arc<Rule>>,
    pub(crate) imports: Vec<String>,
    /// The lookup kinds a provider module answers (`rate`, `quote`, `forecast`).
    pub provides: Vec<LookupKind>,
    /// The declared public API, or `None` for the older rule that every
    /// non-`_` definition is public. See [`Module::public_names`].
    pub(crate) exports: Option<Vec<String>>,
    pub(crate) expressions: Arc<BTreeMap<String, Expr>>,
    pub(crate) hosts: Vec<String>,
    pub(crate) prefix: String,
    pub(crate) properties: Vec<String>,
    pub(crate) cache_key: Option<String>,
    pub(crate) environment: Arc<dyn ModuleEnvironment>,
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
    pub fn revision(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        self.environment.document(&self.path).text.hash(&mut hash);
        for module in &self.environment.modules().modules {
            module.revision().hash(&mut hash);
        }
        format!("{:016x}", hash.finish())
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
    /// trusted the way the Rust adapters are, so the engine contract of
    /// `timer` or `plan` stays callable from `timers` or `plans` while
    /// `exports` keeps it out of notes.
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
        for arg in &args {
            values::check_size(arg)?;
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
    /// enabled?, cache_version?, cache_namespace?, exports?, recognizes?}`.
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
            None => vec![
                Collection::Sections,
                Collection::Tasks,
                Collection::Values,
                Collection::Links,
            ],
            Some(Value::Record(selections)) => {
                for (name, selection) in selections.iter() {
                    fields.insert(name.parse()?, strings(selection)?);
                }
                fields.keys().copied().collect()
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
        // A module reads its own matches whether or not it names them.
        let mut inputs = inputs;
        if !recognizes.is_empty() && !inputs.contains(&Collection::Recognized) {
            inputs.push(Collection::Recognized);
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
                        || ![Hook::Actions, Hook::Hovers, Hook::Diagnostics, Hook::Format]
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
        let lookups: Option<Vec<LookupKind>> = provides.iter().map(|p| p.parse().ok()).collect();
        if kind == ModuleKind::Provider && (provides.is_empty() || lookups.is_none()) {
            return Err(
                "A provider module provides one or more of rate, quote and forecast".into(),
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
            recognizes,
            imports: opt_strings("imports")?,
            provides: lookups.unwrap_or_default(),
            fields,
            hosts,
            prefix,
            properties,
            exports,
            cache_key,
            expressions: Arc::new(expressions),
            environment,
        })
    }
}

/// The most recognizers one module declares.
const MAX_RULES: usize = 16;

/// `recognizes: [{name, on, pattern, tokens?}]`, each pattern compiled and
/// each token checked against the pattern's named groups.
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
        if let Some(key) = fields
            .keys()
            .find(|k| !matches!(k.as_str(), "name" | "on" | "pattern" | "tokens"))
        {
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
        let on: On = text("on")?
            .parse()
            .map_err(|_| format!("Recognizer '{name}': on must be prose, item, heading or row"))?;
        let pattern = common::Pattern::new(&text("pattern")?)
            .map_err(|e| format!("Recognizer '{name}': {e}"))?;
        let mut tokens = Vec::new();
        match fields.get("tokens") {
            None => {}
            Some(Value::Record(paints)) => {
                for (group, paint) in paints.iter() {
                    if !pattern.group_names().any(|g| g == group) {
                        return Err(format!(
                            "Recognizer '{name}' paints '{group}', which its pattern does not name"
                        )
                        .into());
                    }
                    let paint: Paint = match paint {
                        Value::Text(paint) => paint.parse().ok(),
                        _ => None,
                    }
                    .ok_or_else(|| {
                        format!(
                            "Recognizer '{name}': '{group}' must be painted as one of {}",
                            <Paint as strum::VariantNames>::VARIANTS.join(", ")
                        )
                    })?;
                    tokens.push((group.clone(), paint));
                }
            }
            Some(_) => {
                return Err(format!("Recognizer '{name}': tokens must be a record").into());
            }
        }
        rules.push(Arc::new(Rule {
            module: module.into(),
            name,
            on,
            pattern: Arc::new(pattern),
            tokens,
        }));
    }
    Ok(rules)
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
