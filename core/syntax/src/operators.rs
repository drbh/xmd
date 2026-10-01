//! Operators: what `+`, `<=` or `&&` mean between two values, and the operator
//! vocabulary the lexer and the parser share. What each operator computes
//! lives one layer up, in `evaluate::engine::arithmetic`.

use strum::{Display, EnumString, IntoStaticStr};

/// Declare every operator once, with its spelling, so the enum, the lexer's
/// table and the printed form cannot drift apart. The binary ones also name
/// a [`BinaryOp`].
macro_rules! operators {
    (
        binary { $($bop:ident = $bs:literal),* $(,)? }
        other { $($(#[$m:meta])* $op:ident = $s:literal),* $(,)? }
    ) => {
        /// An operator as written, including the ones only the parser gives meaning to.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Operator {
            $($bop,)*
            $($(#[$m])* $op,)*
            /// Lexed, but not part of the language: the parser rejects it where it stands.
            Unsupported(&'static str),
        }
        /// An operation between two values.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum BinaryOp {
            $($bop,)*
        }
        impl Operator {
            /// The operator a lexeme spells, for the finite set of spellings the lexer
            /// can produce.
            pub(super) fn lex(s: &str) -> Option<Self> {
                Some(match s {
                    $($bs => Self::$bop,)*
                    $($s => Self::$op,)*
                    _ => return UNSUPPORTED.iter().find(|u| **u == s).map(|u| Self::Unsupported(u)),
                })
            }
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$bop => $bs,)*
                    $(Self::$op => $s,)*
                    Self::Unsupported(s) => s,
                }
            }
            /// The binary operation this spelling denotes, if any.
            pub fn binary(self) -> Option<BinaryOp> {
                match self {
                    $(Self::$bop => Some(BinaryOp::$bop),)*
                    _ => None,
                }
            }
        }
        impl BinaryOp {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$bop => $bs,)*
                }
            }
        }
    };
}
operators! {
    binary {
        Add = "+", Subtract = "-", Multiply = "*", Divide = "/",
        Equal = "==", NotEqual = "!=", Less = "<", LessEqual = "<=",
        Greater = ">", GreaterEqual = ">=", And = "&&", Or = "||",
    }
    other {
        /// `!`, a prefix operator only.
        Not = "!",
        /// `=>`, between a lambda's parameters and its body.
        Arrow = "=>",
        /// `|`, which calls the function on its right with the value on its left
        /// as the first argument.
        Pipe = "|",
    }
}
/// The spellings the lexer accepts but the language does not.
const UNSUPPORTED: [&str; 8] = ["=", "&", "+=", "-=", "*=", "/=", "&=", "|="];
impl Operator {
    /// The prefix operation this spelling denotes, if any.
    pub(crate) fn unary(self) -> Option<UnaryOp> {
        Some(match self {
            Self::Subtract => UnaryOp::Negate,
            Self::Add => UnaryOp::Plus,
            Self::Not => UnaryOp::Not,
            _ => return None,
        })
    }
}
impl std::fmt::Display for Operator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl std::fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
/// How tightly `|` binds: `xs | length > 0` compares the length, and
/// `a + b | f` pipes the sum.
pub(crate) const PIPE_PRECEDENCE: u8 = 5;
/// How tightly a prefix operator binds, above every binary operator.
pub(crate) const UNARY_PRECEDENCE: u8 = 8;
impl BinaryOp {
    /// Binding power: a higher number binds tighter. A pipe sits between
    /// comparisons and arithmetic, at [`PIPE_PRECEDENCE`].
    pub(crate) fn precedence(self) -> u8 {
        match self {
            Self::Or => 1,
            Self::And => 2,
            Self::Equal | Self::NotEqual => 3,
            Self::Less | Self::LessEqual | Self::Greater | Self::GreaterEqual => 4,
            Self::Add | Self::Subtract => 6,
            Self::Multiply | Self::Divide => 7,
        }
    }
}
/// The comparison at the top of a constraint, as a linear reading reads one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, IntoStaticStr, EnumString, Display)]
pub enum Comparison {
    #[strum(serialize = "<=")]
    LessEqual,
    #[strum(serialize = ">=")]
    GreaterEqual,
    #[strum(serialize = "==")]
    Equal,
}
impl Comparison {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
    /// The comparison an operator makes, for the three a constraint allows.
    pub fn from_op(op: BinaryOp) -> Option<Self> {
        op.as_str().parse().ok()
    }
}
/// A prefix operation on one value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, IntoStaticStr, Display)]
pub enum UnaryOp {
    #[strum(serialize = "-")]
    Negate,
    #[strum(serialize = "+")]
    Plus,
    #[strum(serialize = "!")]
    Not,
}
impl UnaryOp {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}
