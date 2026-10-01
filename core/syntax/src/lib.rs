//! The bottom layer: a note's grammar, with nothing above it. The lexer, the
//! expression tree, the built-in vocabulary, what attribute values hold and scalar
//! literals live here, on top of the shared kernel `common` (which holds
//! `Resource`, value kinds and codes), so `document` can parse a note without
//! reaching into `evaluate` for a single one of them.
mod attributes;
mod builtins;
mod lexer;
mod operators;
mod values;

pub use attributes::{AttributeValue, stamp};
pub use builtins::{Builtin, Tier};
pub use lexer::{Expr, Lexeme, Parser, expression_names, identifier, is_builtin_function, lex};
pub use lexer::{lex_with_comments, sum_scope_at, valid_expression};
pub use operators::{BinaryOp, Comparison, Operator, UnaryOp};
pub use values::{Literal, date_value, duration, is_relative_date, literal, relative_date};
