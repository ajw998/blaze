use chrono::{DateTime, Utc};

use crate::dsl::predicates::Predicate;

#[derive(Debug, Clone)]
pub struct Query {
    pub expr: QueryExpr,
}

/// Boolean expression over leaves.
#[derive(Debug, Clone)]
pub enum QueryExpr {
    And(Vec<QueryExpr>),
    Or(Vec<QueryExpr>),
    Not(Box<QueryExpr>),
    Leaf(LeafExpr),
}

/// Either a free text term or a typed field predicate.
#[derive(Debug, Clone)]
pub enum LeafExpr {
    Text(TextTerm),
    Predicate(Predicate),
}

/// Free-text search term
#[derive(Debug, Clone)]
pub struct TextTerm {
    pub text: String,
    pub is_phrase: bool,
    pub is_glob: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Ext,
    Size,
    Created,
    Modified,
}

/// Comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

/// Typed value for a predicate.
#[derive(Debug, Clone)]
pub enum Value {
    Str(String),
    SizeBytes(u64),
    Time(TimeExpr),
}

/// Time expressions
#[derive(Debug, Clone)]
pub enum TimeExpr {
    Absolute(DateTime<Utc>),
    Relative(RelativeTime),
    Macro(TimeMacro),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelativeTime {
    Days(i64),
    Hours(i64),
    Weeks(i64),
    Years(i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeMacro {
    Today,
    Yesterday,
    ThisWeek,
    LastWeek,
    ThisMonth,
    LastMonth,
}
