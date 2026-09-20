//! One reference model for the whole language, generated from the code that
//! implements it.
//!
//! Nothing here is a second description of a feature: the built-ins and task
//! attributes come from the signature table, the value kinds from `ValueType`,
//! the collections from the catalog's own record structs, the library exports
//! from the `//` comments in `stdlib/*.wtf`, the hooks from `Hook` and the
//! commands from clap. Adding a feature means editing exactly one of those
//! places, and `wtf reference` says so on the next run.
use crate::{
    catalog::Collection,
    engine::{Builtin, ValueType},
    modules::{Hook, Module, ModuleKind, ModuleRegistry, bundled},
    signature::{BUILTINS, Group, Signature, Tier},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The schema the hosts and the docs code against.
pub const VERSION: u64 = 2;

/// The seven constructs a note is written with, as `(syntax, meaning)`: the
/// page and the Markdown both open on this table, so it lives in the model
/// rather than in either of them.
pub const SYNTAXES: &[(&str, &str)] = &[
    ("$1,234:car", "a value with a name"),
    ("total := car + $67", "a calculation"),
    ("[total]", "any value in a sentence"),
    ("2026-11-20:departure", "a date"),
    (
        "- [ ] pack @due(departure - 14d)",
        "a task with an attribute",
    ),
    ("## trip :trip", "a named heading that counts its tasks"),
    ("focus := countdown(25m)", "a timer"),
];

/// The value kinds a note author meets as literals or results, in the order
/// the reference reads them. Every other kind is an object the engine builds
/// and a note reads through properties.
const SCALARS: &[ValueType] = &[
    ValueType::Number,
    ValueType::Count,
    ValueType::Money,
    ValueType::Ratio,
    ValueType::Duration,
    ValueType::Date,
    ValueType::DateTime,
    ValueType::Boolean,
    ValueType::Text,
    ValueType::List,
    ValueType::Record,
    ValueType::Null,
];

/// The whole reference, as JSON. The top level is what a note author needs;
/// everything a .wtf module is written with sits under `authoring`.
pub fn model(modules: &ModuleRegistry) -> Value {
    json!({
        "version": VERSION,
        "syntaxes": syntaxes(),
        "functions": functions(|tier| tier != Tier::Module),
        "attributes": attributes(),
        "types": types(),
        "collections": collections("query"),
        "library": library(modules),
        "commands": commands(),
        "authoring": {
            "functions": functions(|tier| tier == Tier::Module),
            "collections": collections("module"),
            "modules": self::modules(modules),
            "hooks": hooks(),
        },
    })
}

fn syntaxes() -> Vec<Value> {
    SYNTAXES
        .iter()
        .map(|(syntax, meaning)| json!({ "syntax": syntax, "meaning": meaning }))
        .collect()
}

/// The built-ins of the chosen tiers, in `Builtin::ALL` order, which is the
/// order the editor offers them in. `example` is signature help's own and
/// stays out: `try` is the snippet a reader runs.
fn functions(chosen: impl Fn(Tier) -> bool) -> Vec<Value> {
    table()
        .take(Builtin::ALL.len())
        .filter(|f| chosen(f.tier))
        .map(|f| {
            json!({
                "name": f.name,
                "group": f.group.as_str(),
                "tier": f.tier.as_str(),
                "params": f.params,
                "result": f.result.as_str(),
                "documentation": f.documentation,
                "try": f.try_,
            })
        })
        .collect()
}

/// The task and appointment attributes, which are written like calls but name
/// no built-in function.
fn attributes() -> Vec<Value> {
    table()
        .skip(Builtin::ALL.len())
        .map(|a| {
            json!({
                "name": a.name,
                "params": a.params,
                "documentation": a.documentation,
                "try": a.try_,
            })
        })
        .collect()
}

fn table() -> impl Iterator<Item = &'static Signature> {
    BUILTINS.iter()
}

/// The scalar kinds first, in `SCALARS` order, then the engine's objects in
/// declaration order; `tier` says which a row is.
fn types() -> Vec<Value> {
    SCALARS
        .iter()
        .map(|kind| (kind, "scalar"))
        .chain(objects().map(|kind| (kind, "object")))
        .map(|(kind, tier)| {
            json!({
                "name": kind.as_str(),
                "tier": tier,
                "fields": kind.fields(),
                "documentation": kind.documentation(),
                "try": kind.try_snippet(),
            })
        })
        .collect()
}

/// The kinds the engine builds: a note reads them through properties and
/// never writes one as a literal.
fn objects() -> impl Iterator<Item = &'static ValueType> {
    ValueType::ALL.iter().filter(|kind| !SCALARS.contains(kind))
}

fn collections(tier: &str) -> Vec<Value> {
    Collection::ALL
        .iter()
        .filter(|collection| collection.tier() == tier)
        .map(|collection| {
            json!({
                "name": collection.as_str(),
                "tier": collection.tier(),
                "fields": collection.fields(),
                "documentation": collection.documentation(),
                "try": collection.try_snippet(),
            })
        })
        .collect()
}

fn hooks() -> Vec<Value> {
    Hook::ALL
        .iter()
        .map(|hook| {
            json!({
                "name": hook.name(),
                "kind": hook.kinds(),
                "arity": hook.arity(),
                "documentation": hook.documentation(),
            })
        })
        .collect()
}

/// Every module the registry holds, plus any bundled module a workspace
/// manifest has not replaced.
fn all_modules(modules: &ModuleRegistry) -> Vec<&Module> {
    let known: Vec<&str> = modules.modules.iter().map(|m| m.id.as_str()).collect();
    modules
        .modules
        .iter()
        .chain(bundled().iter().filter(|m| !known.contains(&m.id.as_str())))
        .collect()
}

/// The libraries a note can `import`, each with the members it exports.
/// Libraries a note can import. One that exports nothing is the engine's own
/// (`timer`, `plan`, `itinerary_core`) and belongs with the bundled modules.
fn library(modules: &ModuleRegistry) -> Vec<Value> {
    all_modules(modules)
        .into_iter()
        .filter(|m| m.kind == ModuleKind::Library && !m.public_names().is_empty())
        .map(described)
        .collect()
}

/// The modules a note never names: link and feature modules, whose hooks the
/// host calls, and libraries that export nothing because the engine calls
/// them by name. The reference says what each one is and nothing of how.
fn modules(modules: &ModuleRegistry) -> Vec<Value> {
    all_modules(modules)
        .into_iter()
        .filter(|m| m.kind != ModuleKind::Library || m.public_names().is_empty())
        .map(|module| {
            let mut entry = json!({
                "id": module.id,
                "kind": module.kind.as_str(),
                "documentation": module_documentation(module.source()),
            });
            if module.kind == ModuleKind::Link {
                entry["hosts"] = json!(module.hosts());
            }
            entry
        })
        .collect()
}

fn described(module: &Module) -> Value {
    let source = module.source();
    // The module says what its API is; the source only supplies the comment
    // and parameter list for each name it exports.
    let mut functions: BTreeMap<String, Export> = exports(source)
        .into_iter()
        .map(|export| (export.name.clone(), export))
        .collect();
    let exports: Vec<Value> = module
        .public_names()
        .iter()
        .filter_map(|name| functions.remove(name))
        .map(|export| {
            // A library is reached by name, so each export shows a call a
            // reader can paste.
            let arguments = export
                .params
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            json!({
                "name": export.name,
                "params": export.params,
                "documentation": export.documentation,
                "try": format!(
                    "lib := import(\"{}\")\nresult := lib.{}({arguments})",
                    module.id, export.name
                ),
            })
        })
        .collect();
    json!({
        "id": module.id,
        "kind": module.kind.as_str(),
        "documentation": module_documentation(source),
        "exports": exports,
    })
}

struct Export {
    name: String,
    params: Vec<String>,
    documentation: String,
}

/// The `//` block immediately above `module :=`, or, failing that, the first
/// comment block above it. A comment further down belongs to an export, so a
/// module that says nothing about itself reports nothing.
fn module_documentation(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.trim_start().starts_with("module :="))
        .unwrap_or(lines.len());
    let above = comment_above(&lines, at);
    if !above.is_empty() {
        return above;
    }
    let first = lines[..at]
        .iter()
        .position(|line| line.trim_start().starts_with("//"));
    match first {
        Some(first) => {
            let end = lines[first..at]
                .iter()
                .position(|line| !line.trim_start().starts_with("//"))
                .map_or(at, |end| first + end);
            comment_above(&lines, end)
        }
        None => String::new(),
    }
}

/// The contiguous `//` lines directly above `index`, joined into one line.
fn comment_above(lines: &[&str], index: usize) -> String {
    let mut collected: Vec<&str> = Vec::new();
    for line in lines[..index].iter().rev() {
        let trimmed = line.trim_start();
        if let Some(text) = trimmed.strip_prefix("//") {
            collected.push(text.trim());
        } else {
            break;
        }
    }
    collected.reverse();
    collected.join(" ")
}

/// Every top-level `name := fn(params) => …`, with the comment above it,
/// whether or not the module exports it; the caller keeps the exported ones.
fn exports(source: &str) -> Vec<Export> {
    let lines: Vec<&str> = source.lines().collect();
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        // Top level only: an indented line is inside another definition.
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        let Some((name, rest)) = line.split_once(":=") else {
            continue;
        };
        let name = name.trim();
        let rest = rest.trim_start();
        if name.starts_with('_')
            || name == "module"
            || name.is_empty()
            || !crate::document::identifier(name)
        {
            continue;
        }
        let Some(params) = rest.strip_prefix("fn(").and_then(|r| r.split_once(')')) else {
            continue;
        };
        found.push(Export {
            name: name.to_string(),
            params: params
                .0
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect(),
            documentation: comment_above(&lines, index),
        });
    }
    found
}

/// The command line, straight from clap, so a new subcommand documents itself.
#[cfg(feature = "native")]
fn commands() -> Vec<Value> {
    use clap::CommandFactory;
    let mut cli = crate::cli::Cli::command();
    cli.build();
    cli.get_subcommands()
        // `help` is clap's own, not one of the language's commands.
        .filter(|command| command.get_name() != "help")
        .map(|command| {
            let usage = command.clone().render_usage().to_string();
            json!({
                "name": command.get_name(),
                "usage": usage.trim_start_matches("Usage: ").to_string(),
                "documentation": command
                    .get_about()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            })
        })
        .collect()
}
#[cfg(not(feature = "native"))]
fn commands() -> Vec<Value> {
    Vec::new()
}

/// The same reference as dense Markdown: what a person or an LLM reads in a
/// terminal. The order is the page's: the syntax first, then what a note
/// author reaches for, and module authoring last.
pub fn markdown(modules: &ModuleRegistry) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "# wtf reference (v{VERSION})\n");

    let _ = writeln!(out, "## Syntax\n");
    let _ = writeln!(out, "| syntax | meaning |");
    let _ = writeln!(out, "| --- | --- |");
    for (syntax, meaning) in SYNTAXES {
        let _ = writeln!(out, "| `{syntax}` | {meaning} |");
    }
    out.push('\n');

    // A note's own built-ins first, then the toolkit it reaches for once it
    // needs lists and text. The module tier is not part of the language a note
    // writes at all, so it waits for the last section.
    let _ = writeln!(out, "## Functions\n");
    groups(&mut out, Tier::Note, "###");
    let _ = writeln!(out, "### Toolkit\n");
    groups(&mut out, Tier::Toolkit, "####");

    let _ = writeln!(out, "## Attributes\n");
    for a in BUILTINS.iter().skip(Builtin::ALL.len()) {
        let _ = writeln!(out, "`{}({})`", a.name, a.params.join(", "));
        let _ = writeln!(out, "{}\n", a.documentation);
        let _ = writeln!(out, "```wtf\n{}\n```\n", a.try_);
    }

    // The scalars a note writes, then the objects it only reads.
    let _ = writeln!(out, "## Types\n");
    for kind in SCALARS {
        describe_type(&mut out, *kind);
    }
    let _ = writeln!(out, "### Engine objects\n");
    for kind in objects() {
        describe_type(&mut out, *kind);
    }

    let _ = writeln!(out, "## Query\n");
    let _ = writeln!(out, "### Collections\n");
    for collection in Collection::ALL.iter().filter(|c| c.tier() == "query") {
        describe_collection(&mut out, *collection);
    }
    let _ = writeln!(out, "### Commands\n");
    for command in commands() {
        let _ = writeln!(
            out,
            "`{}` — {}",
            command["name"].as_str().unwrap_or_default(),
            command["documentation"].as_str().unwrap_or_default()
        );
        for line in command["usage"].as_str().unwrap_or_default().lines() {
            let _ = writeln!(out, "    {}", line.trim());
        }
    }
    out.push('\n');

    // Only what a note can import: each library and its exports.
    let _ = writeln!(out, "## Library\n");
    for module in library(modules) {
        let _ = writeln!(out, "### {}\n", module["id"].as_str().unwrap_or_default());
        let documentation = module["documentation"].as_str().unwrap_or_default();
        if !documentation.is_empty() {
            let _ = writeln!(out, "{documentation}\n");
        }
        let exports = module["exports"].as_array().cloned().unwrap_or_default();
        for export in &exports {
            let params: Vec<&str> = export["params"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let _ = writeln!(
                out,
                "- `{}({})` {}",
                export["name"].as_str().unwrap_or_default(),
                params.join(", "),
                export["documentation"].as_str().unwrap_or_default()
            );
        }
        out.push('\n');
    }

    // Everything a .wtf module is written with, and nothing a note can use.
    let _ = writeln!(out, "## Module authoring\n");
    let _ = writeln!(out, "### Functions\n");
    groups(&mut out, Tier::Module, "####");
    let _ = writeln!(out, "### Collections\n");
    for collection in Collection::ALL.iter().filter(|c| c.tier() == "module") {
        describe_collection(&mut out, *collection);
    }
    let _ = writeln!(out, "### Bundled modules\n");
    let _ = writeln!(out, "| id | kind | about |");
    let _ = writeln!(out, "| --- | --- | --- |");
    for module in self::modules(modules) {
        let _ = writeln!(
            out,
            "| {} | {} | {} |",
            module["id"].as_str().unwrap_or_default(),
            module["kind"].as_str().unwrap_or_default(),
            module["documentation"].as_str().unwrap_or_default()
        );
    }
    out.push('\n');
    let _ = writeln!(out, "### Hooks\n");
    for hook in Hook::ALL {
        let _ = writeln!(
            out,
            "- `{}/{}` ({}) {}",
            hook.name(),
            hook.arity(),
            hook.kinds(),
            hook.documentation()
        );
    }
    out.push('\n');
    out
}

/// One value kind, with the fields every value of it carries and a note that
/// produces one.
fn describe_type(out: &mut String, kind: ValueType) {
    use std::fmt::Write as _;
    let fields = kind.fields();
    let _ = writeln!(
        out,
        "`{}`{}",
        kind.as_str(),
        if fields.is_empty() {
            String::new()
        } else {
            format!(" — fields: {}", fields.join(", "))
        }
    );
    let _ = writeln!(out, "{}\n", kind.documentation());
    let _ = writeln!(out, "```wtf\n{}\n```\n", kind.try_snippet());
}

/// One collection, with the fields its records carry and a query to paste.
fn describe_collection(out: &mut String, collection: Collection) {
    use std::fmt::Write as _;
    let _ = writeln!(
        out,
        "`{}` — fields: {}",
        collection.as_str(),
        collection.fields().join(", ")
    );
    let _ = writeln!(out, "{}\n", collection.documentation());
    let _ = writeln!(out, "```wtf\n{}\n```\n", collection.try_snippet());
}

/// One tier's built-ins, in the reference's section order, under headings of
/// the given depth.
fn groups(out: &mut String, tier: Tier, heading: &str) {
    use std::fmt::Write as _;
    for group in Group::ALL {
        let members: Vec<&Signature> = BUILTINS
            .iter()
            .take(Builtin::ALL.len())
            .filter(|f| f.tier == tier && f.group == *group)
            .collect();
        if members.is_empty() {
            continue;
        }
        let _ = writeln!(out, "{heading} {}\n", group.as_str());
        for f in members {
            let _ = writeln!(
                out,
                "`{}({}) -> {}`",
                f.name,
                f.params.join(", "),
                f.result.as_str()
            );
            let _ = writeln!(out, "{}\n", f.documentation);
            let _ = writeln!(out, "```wtf\n{}\n```\n", f.try_);
        }
    }
}

/// Every `try` snippet that is a note, as `(name, source)` pairs: the
/// functions, the attributes and the value kinds. Writing them out and
/// querying their diagnostics is how the suite proves the docs still run.
/// A module-tier snippet is module code, not a note, so it is not among them.
pub fn snippets() -> Vec<(String, &'static str)> {
    let mut all: Vec<(String, &'static str)> = BUILTINS
        .iter()
        .take(Builtin::ALL.len())
        .filter(|f| f.tier != Tier::Module)
        .map(|f| (format!("fn-{}", f.name), f.try_))
        .collect();
    all.extend(
        BUILTINS
            .iter()
            .skip(Builtin::ALL.len())
            .map(|a| (format!("attr-{}", a.name.trim_start_matches('@')), a.try_)),
    );
    all.extend(ValueType::ALL.iter().map(|kind| {
        (
            format!("type-{}", kind.as_str().to_lowercase()),
            kind.try_snippet(),
        )
    }));
    all
}
