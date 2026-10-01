//! Operators: what `+`, `<=` or `&&` mean between two values, and the operator
//! vocabulary the lexer and the parser share. What each operator computes
//! lives one layer up, in `evaluate::engine::arithmetic`.

/// An operator as written, including the ones only the parser gives meaning to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
    /// `!`, a prefix operator only.
    Not,
    /// `=>`, between a lambda's parameters and its body.
    Arrow,
    /// `|`, which calls the function on its right with the value on its left
    /// as the first argument.
    Pipe,
    /// Lexed, but not part of the language: the parser rejects it where it stands.
    Unsupported(&'static str),
}
impl Operator {
    /// The operator a lexeme spells, for the finite set of spellings the lexer
    /// can produce.
    pub(super) fn lex(s: &str) -> Option<Self> {
        Some(match s {
            "+" => Self::Add,
            "-" => Self::Subtract,
            "*" => Self::Multiply,
            "/" => Self::Divide,
            "==" => Self::Equal,
            "!=" => Self::NotEqual,
            "<" => Self::Less,
            "<=" => Self::LessEqual,
            ">" => Self::Greater,
            ">=" => Self::GreaterEqual,
            "&&" => Self::And,
            "||" => Self::Or,
            "!" => Self::Not,
            "=>" => Self::Arrow,
            "|" => Self::Pipe,
            "=" => Self::Unsupported("="),
            "&" => Self::Unsupported("&"),
            "+=" => Self::Unsupported("+="),
            "-=" => Self::Unsupported("-="),
            "*=" => Self::Unsupported("*="),
            "/=" => Self::Unsupported("/="),
            "&=" => Self::Unsupported("&="),
            "|=" => Self::Unsupported("|="),
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Less => "<",
            Self::LessEqual => "<=",
            Self::Greater => ">",
            Self::GreaterEqual => ">=",
            Self::And => "&&",
            Self::Or => "||",
            Self::Not => "!",
            Self::Arrow => "=>",
            Self::Pipe => "|",
            Self::Unsupported(s) => s,
        }
    }
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
/// How tightly `|` binds: `xs | length > 0` compares the length, and
/// `a + b | f` pipes the sum.
pub(crate) const PIPE_PRECEDENCE: u8 = 5;
/// How tightly a prefix operator binds, above every binary operator.
pub(crate) const UNARY_PRECEDENCE: u8 = 8;
/// Declare the binary operations once, each named as the operator that
/// spells it, so the two lists cannot drift apart.
macro_rules! binary_ops {
    ($($op:ident),*) => {
        /// An operation between two values.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum BinaryOp {
            $($op,)*
        }
        impl Operator {
            /// The binary operation this spelling denotes, if any.
            pub fn binary(self) -> Option<BinaryOp> {
                match self {
                    $(Self::$op => Some(BinaryOp::$op),)*
                    _ => None,
                }
            }
        }
        impl BinaryOp {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$op => Operator::$op.as_str(),)*
                }
            }
        }
        impl std::fmt::Display for BinaryOp {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}
binary_ops! {
    Add, Subtract, Multiply, Divide, Equal, NotEqual,
    Less, LessEqual, Greater, GreaterEqual, And, Or
}
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
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr, strum::EnumString, strum::Display,
)]
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
        match op {
            BinaryOp::LessEqual => Some(Self::LessEqual),
            BinaryOp::GreaterEqual => Some(Self::GreaterEqual),
            BinaryOp::Equal => Some(Self::Equal),
            _ => None,
        }
    }
}
/// A prefix operation on one value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr, strum::Display)]
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
