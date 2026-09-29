//! How every feature names a symbol: the kind and the one-line detail the
//! outline, the call hierarchy and completion all show, so the three agree.
use lang::eval::engine::{Engine, Value};
use lang::eval::plans::PlanValue;
use lang::eval::tables::TableValue;
use lang::eval::{EvalResult, Symbol, SymbolKind};
use lang::model::Document;
use lsp_types::SymbolKind as LspSymbolKind;

pub(crate) fn kind(doc: &Document, symbol: &Symbol) -> LspSymbolKind {
    match symbol.kind {
        SymbolKind::Task(_) => LspSymbolKind::BOOLEAN,
        SymbolKind::Section(_) => LspSymbolKind::NAMESPACE,
        SymbolKind::Column(..) => LspSymbolKind::FIELD,
        SymbolKind::Variable(..) => LspSymbolKind::VARIABLE,
        SymbolKind::Definition(i) if doc.table_of(i).is_some() || doc.plan_of(i).is_some() => {
            LspSymbolKind::STRUCT
        }
        SymbolKind::Definition(i) if doc.definitions[i].expression => LspSymbolKind::VARIABLE,
        SymbolKind::Definition(_) => LspSymbolKind::CONSTANT,
    }
}

pub fn detail(engine: &mut Engine<'_>, doc: &Document, symbol: &Symbol) -> String {
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
            Ok(table) if table.downcast::<TableValue>().is_some() => {
                format!("table · {}", table.display())
            }
            Ok(ref value) if let Some(p) = value.downcast::<PlanValue>() => format!(
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
pub fn summary(value: &EvalResult<Value>) -> String {
    match value {
        Ok(v) => format!("{} · {}", v.type_name(), v.display()),
        Err(e) => format!("Error · {e}"),
    }
}
