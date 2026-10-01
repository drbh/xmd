//! The registry of feature evaluators: the one list of what each feature
//! answers for the engine. The engine evaluates expressions, names, calls and
//! the core special forms (`if`, `match`, `let`, `eval`, `import`, …); a
//! definition the model knows as a feature ([`DefinitionKind`]) and a special
//! built-in the engine does not answer itself are looked up here, so the
//! engine never names a feature and every feature depends on the engine
//! rather than the other way round. A new feature is one arm.
//!
//! One generic form sits beside the arms: [`adopt`], which hands a tagged
//! record that asks for it the definition whose call built it.
use crate::engine::{Builtin, Engine, Expr, Parser, Value};
use crate::workspace::Symbol;
use crate::{forms, tables_impl};
use model::{DefinitionKind, Document};
use std::path::Path;
use values::{EvalResult, record};

/// Evaluate definition `index` of `symbol`'s note, which the model knows as
/// the kind the arm names.
pub(crate) type Evaluate = fn(&mut Engine<'_>, &Symbol, usize) -> EvalResult<Value>;
/// Answer a special built-in: it decides for itself what to evaluate.
pub(crate) type Call = fn(&mut Engine<'_>, &Path, &[Expr]) -> EvalResult<Value>;

/// The feature that evaluates definitions of `kind`, or `None` for the
/// expressions and literals the engine reads itself. Everything else a note
/// means is the stdlib's, over the records `checklist.tasks` and `cached`
/// read, `tagged` records and `clocked`.
pub(crate) fn evaluator(kind: DefinitionKind) -> Option<Evaluate> {
    match kind {
        // A definition that calls a form a module declares: its expressions
        // read as the form says, and the module's `define` says what it is
        // worth.
        DefinitionKind::Form => Some(forms::evaluate),
        DefinitionKind::Table => Some(tables_impl::evaluate),
        _ => None,
    }
}

/// The feature that answers a special built-in the engine does not answer
/// itself, if any does.
pub(crate) fn call(builtin: Builtin) -> Option<Call> {
    match builtin {
        // The row `sum(table, row expression)` that walks a table.
        Builtin::Sum => Some(tables_impl::call_sum),
        _ => None,
    }
}

/// The value expression definition `symbol` evaluated to, claimed by that
/// definition when it is a tagged record that asks where it was made
/// ([`values::claim`]) and the definition's whole expression is a call. The
/// record's `origin` becomes `{document, name, line, text, range, function,
/// arguments}`: the definition's note (as a URI), its name, the line it is
/// named on and that line as written, the range of its expression, and the
/// function the expression calls with the source of each argument. Any other
/// value is handed back unchanged.
pub(crate) fn adopt(value: Value, symbol: &Symbol, doc: &Document) -> Value {
    values::claim(&value, || origin(symbol, doc)).unwrap_or(value)
}

/// Where definition `symbol` of `doc` made a value, as [`adopt`] describes
/// it, or `None` when its expression is not a call.
fn origin(symbol: &Symbol, doc: &Document) -> Option<Value> {
    let crate::workspace::SymbolKind::Definition(index) = symbol.kind else {
        return None;
    };
    let definition = &doc.definitions[index];
    let parsed = Parser::parse(&definition.source).ok()?;
    let Expr::Call(function, arguments) = parsed.bare() else {
        return None;
    };
    let raw = definition.value_span.source(doc);
    let leading = raw.len() - raw.trim_start().len();
    let trailing = raw.len() - leading - raw.trim().len();
    let span = common::Span::new(
        definition.value_span.line,
        definition.value_span.start + leading,
        definition.value_span.end - trailing,
    );
    let range = span.range(doc);
    let position = |line: u32, character: u32| {
        record([
            ("line", Value::Count(line as usize)),
            ("character", Value::Count(character as usize)),
        ])
    };
    let line = definition.named.span.line;
    Some(record([
        ("document", Value::Text(crate::forms::uri(&symbol.path))),
        ("name", Value::Text(definition.named.name.clone())),
        ("line", Value::Count(line)),
        ("text", Value::Text(doc.line(line).into())),
        (
            "range",
            record([
                ("start", position(range.start.line, range.start.character)),
                ("end", position(range.end.line, range.end.character)),
            ]),
        ),
        ("function", Value::Text(function.clone())),
        (
            "arguments",
            Value::list(
                arguments
                    .iter()
                    .map(|argument| {
                        let (start, end) = argument.bounds();
                        Value::Text(definition.source[start..end].trim().into())
                    })
                    .collect(),
            ),
        ),
    ]))
}
