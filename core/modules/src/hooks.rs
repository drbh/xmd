//! The hooks, records and `step` loops a module meets, declared as data in
//! `data/hooks.md`: what the host hands each hook and each step and what must
//! come back. That file, read once on first use, generates
//! `book/reference/contract.md`; its opening paragraphs give its format.
use crate::module::{Hook, ModuleKind};
use std::sync::LazyLock;
use strum::VariantArray;

/// One hook as the host calls it: which kinds of module it belongs to, what
/// goes in, and what has to come back. The other half of the stdlib contract:
/// there native code calls the library, here it calls a module's hooks.
#[derive(Clone, Debug)]
pub struct HookContract {
    pub hook: Hook,
    /// The kinds of module the host calls it on.
    pub kinds: Vec<ModuleKind>,
    /// Each parameter as `name: shape`, one per argument the hook takes. A lowercase
    /// shape names a record in [`HOOK_RECORDS`] or a side of a [`StepProtocol`].
    pub params: Vec<&'static str>,
    pub returns: &'static str,
    pub doc: &'static str,
}

/// A record the host builds for more than one hook, declared once.
#[derive(Clone, Debug)]
pub struct HookRecord {
    pub name: &'static str,
    pub doc: &'static str,
    /// Each field as `name: shape and meaning`, the way
    /// [`HookContract::params`] are written; a `?` after the name marks one
    /// that may be absent.
    pub fields: Vec<&'static str>,
}

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
#[derive(Clone, Debug)]
pub struct StepProtocol {
    pub kind: ModuleKind,
    pub doc: &'static str,
    /// Each field of the step's input, as `name: shape and meaning`.
    pub input: Vec<&'static str>,
    /// Each field of the record a step returns; `?` marks an optional one.
    pub output: Vec<&'static str>,
    pub effects: Vec<Effect>,
}

/// The records hooks are handed and the actions they return, by name.
pub static HOOK_RECORDS: LazyLock<Vec<HookRecord>> = LazyLock::new(|| {
    entries("records")
        .map(|entry| HookRecord {
            name: entry.title,
            doc: entry.doc,
            fields: entry.list(""),
        })
        .collect()
});

/// Every hook, in [`Hook`] order.
pub static HOOKS: LazyLock<Vec<HookContract>> = LazyLock::new(|| {
    entries("hooks")
        .map(|entry| {
            let (name, kinds) = entry
                .title
                .split_once(": ")
                .expect("a hook names its kinds");
            HookContract {
                hook: *Hook::VARIANTS
                    .iter()
                    .find(|h| h.as_ref() == name)
                    .expect(name),
                kinds: kinds.split(", ").map(kind).collect(),
                params: entry.list(""),
                returns: entry.returns,
                doc: entry.doc,
            }
        })
        .collect()
});

/// The `step` loops of commands and providers.
pub static STEPS: LazyLock<Vec<StepProtocol>> = LazyLock::new(|| {
    let effect = |line: &'static str| {
        let (kind, line) = line.split_once(' ')?;
        let (request, line) = line.split_once(" -> ")?;
        let (answer, doc) = line.split_once(": ")?;
        Some(Effect {
            kind,
            request,
            answer,
            doc,
        })
    };
    entries("steps")
        .map(|entry| StepProtocol {
            kind: kind(entry.title),
            doc: entry.doc,
            input: entry.list("input"),
            output: entry.list("output"),
            effects: (entry.list("effects").into_iter())
                .map(|line| effect(line).expect("an effect is `kind request -> answer: doc`"))
                .collect(),
        })
        .collect()
});

fn kind(name: &str) -> ModuleKind {
    name.parse().expect("a module kind")
}

/// One `##` entry of `data/hooks.md`: its heading, its line of prose, its
/// `returns` line, and its `- ` lines, each under the `###` heading above it
/// (`""` before any).
#[derive(Default)]
struct Entry {
    title: &'static str,
    doc: &'static str,
    returns: &'static str,
    items: Vec<(&'static str, &'static str)>,
}
impl Entry {
    fn list(&self, under: &str) -> Vec<&'static str> {
        let items = self.items.iter().filter(|(heading, _)| *heading == under);
        items.map(|&(_, item)| item).collect()
    }
}

/// The entries of the `# {part}` part of `data/hooks.md`.
fn entries(part: &str) -> impl Iterator<Item = Entry> {
    let text = include_str!("../data/hooks.md");
    let part = (text.split("\n# "))
        .find_map(|p| p.strip_prefix(part)?.strip_prefix('\n'))
        .expect("hooks.md has the part");
    part.split("\n## ").skip(1).map(|section| {
        let mut lines = section.lines().filter(|line| !line.trim().is_empty());
        let mut entry = Entry {
            title: lines.next().unwrap_or_default(),
            ..Entry::default()
        };
        let mut under = "";
        for line in lines {
            if let Some(heading) = line.strip_prefix("### ") {
                under = heading;
            } else if let Some(item) = line.strip_prefix("- ") {
                entry.items.push((under, item));
            } else if let Some(returns) = line.strip_prefix("returns ") {
                entry.returns = returns;
            } else {
                entry.doc = line;
            }
        }
        entry
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert!(
                !contract.returns.is_empty() && !contract.doc.is_empty(),
                "{hook}"
            );
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
        for protocol in STEPS.iter() {
            assert!(!protocol.doc.is_empty() && !protocol.effects.is_empty());
        }
    }

    /// Every record has its prose and fields.
    #[test]
    fn every_record_is_described() {
        assert!(!HOOK_RECORDS.is_empty());
        for record in HOOK_RECORDS.iter() {
            assert!(
                !record.doc.is_empty() && !record.fields.is_empty(),
                "{}",
                record.name
            );
        }
    }
}
