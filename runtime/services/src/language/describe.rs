//! How every feature names a symbol: the kind and the one-line detail the
//! outline, the call hierarchy and completion all show, so the three agree.
use lang::eval::engine::{Engine, Value};
use lang::eval::{EvalResult, Symbol, SymbolKind};
use lang::model::Document;
use lsp_types::SymbolKind as Kind;

pub(crate) fn kind(doc: &Document, symbol: &Symbol) -> Kind {
    match symbol.kind {
        SymbolKind::Task(_) => Kind::BOOLEAN,
        SymbolKind::Section(_) => Kind::NAMESPACE,
        SymbolKind::Column(..) => Kind::FIELD,
        SymbolKind::Variable(..) => Kind::VARIABLE,
        SymbolKind::Definition(i) if doc.table_of(i).is_some() || doc.plan_of(i).is_some() => {
            Kind::STRUCT
        }
        SymbolKind::Definition(i) if doc.definitions[i].expression => Kind::VARIABLE,
        SymbolKind::Definition(_) => Kind::CONSTANT,
    }
}

pub(crate) fn detail(engine: &mut Engine<'_>, doc: &Document, symbol: &Symbol) -> String {
    match symbol.kind {
        SymbolKind::Task(i) => {
            let blocked = engine.blocked(&symbol.path, i);
            match blocked {
                _ if engine.task_done(&symbol.path, i) => "task · complete".into(),
                Ok(names) if !names.is_empty() => format!("task · blocked by {}", names.join(", ")),
                Ok(_) => "task · incomplete".into(),
                Err(e) => format!("task · {e}"),
            }
        }
        SymbolKind::Section(_) => match engine.symbol(symbol) {
            Ok(Value::Tasks(tasks)) => {
                let done = tasks
                    .iter()
                    .filter(|(p, i)| engine.task_done(p, *i))
                    .count();
                format!("checklist · {done}/{} complete", tasks.len())
            }
            other => summary(&other),
        },
        SymbolKind::Column(t, c) => format!(
            "{} · column of {}",
            doc.tables[t].types[c]
                .map(|t| t.as_str())
                .unwrap_or("Unknown"),
            doc.definitions[doc.tables[t].definition].named.name
        ),
        SymbolKind::Variable(p, _) => match engine.symbol(symbol) {
            Ok(v) => format!(
                "decision variable of {} · {}",
                doc.definitions[doc.plans[p].definition].named.name,
                v.display()
            ),
            Err(e) => format!("decision variable · {e}"),
        },
        SymbolKind::Definition(_) => match engine.symbol(symbol) {
            Ok(table @ Value::Table(_)) => format!("table · {}", table.display()),
            Ok(Value::Plan(p)) => format!(
                "plan · {} {} · {} variables",
                p.goal.keyword(),
                p.objective.display(),
                p.variables.len()
            ),
            other => summary(&other),
        },
    }
}

/// A value as a one-line detail: its type and how a note shows it.
pub(crate) fn summary(value: &EvalResult<Value>) -> String {
    match value {
        Ok(v) => format!("{} · {}", v.type_name(), v.display()),
        Err(e) => format!("Error · {e}"),
    }
}
