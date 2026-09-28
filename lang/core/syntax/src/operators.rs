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
    /// `|`, the query pipeline separator; expressions have no use for it.
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
    /// The binary operation this spelling denotes, if any.
    pub fn binary(self) -> Option<BinaryOp> {
        Some(match self {
            Self::Add => BinaryOp::Add,
            Self::Subtract => BinaryOp::Subtract,
            Self::Multiply => BinaryOp::Multiply,
            Self::Divide => BinaryOp::Divide,
            Self::Equal => BinaryOp::Equal,
            Self::NotEqual => BinaryOp::NotEqual,
            Self::Less => BinaryOp::Less,
            Self::LessEqual => BinaryOp::LessEqual,
            Self::Greater => BinaryOp::Greater,
            Self::GreaterEqual => BinaryOp::GreaterEqual,
            Self::And => BinaryOp::And,
            Self::Or => BinaryOp::Or,
            _ => return None,
        })
    }
    /// The prefix operation this spelling denotes, if any.
    pub fn unary(self) -> Option<UnaryOp> {
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
/// An operation between two values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
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
}
impl BinaryOp {
    pub fn as_str(self) -> &'static str {
        self.operator().as_str()
    }
    pub fn operator(self) -> Operator {
        match self {
            Self::Add => Operator::Add,
            Self::Subtract => Operator::Subtract,
            Self::Multiply => Operator::Multiply,
            Self::Divide => Operator::Divide,
            Self::Equal => Operator::Equal,
            Self::NotEqual => Operator::NotEqual,
            Self::Less => Operator::Less,
            Self::LessEqual => Operator::LessEqual,
            Self::Greater => Operator::Greater,
            Self::GreaterEqual => Operator::GreaterEqual,
            Self::And => Operator::And,
            Self::Or => Operator::Or,
        }
    }
    /// Binding power: a higher number binds tighter.
    pub fn precedence(self) -> u8 {
        match self {
            Self::Or => 1,
            Self::And => 2,
            Self::Equal | Self::NotEqual => 3,
            Self::Less | Self::LessEqual | Self::Greater | Self::GreaterEqual => 4,
            Self::Add | Self::Subtract => 5,
            Self::Multiply | Self::Divide => 6,
        }
    }
}
impl std::fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
/// The comparison at the top of a plan constraint.
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    Plus,
    Not,
}
impl UnaryOp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Negate => "-",
            Self::Plus => "+",
            Self::Not => "!",
        }
    }
}
impl std::fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
