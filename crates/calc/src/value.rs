//! Cell values and spreadsheet error codes.

use std::fmt;

/// Spreadsheet error values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// Division by zero.
    DivZero,
    /// Wrong argument type.
    Value,
    /// Invalid cell reference.
    Ref,
    /// Unknown function or name.
    Name,
    /// Value not available.
    NA,
    /// Invalid numeric result or argument.
    Num,
    /// Circular reference.
    Circular,
    /// Formula could not be parsed.
    Syntax,
    /// Wrong number of arguments.
    Args,
}

impl ErrorKind {
    /// The text shown in a cell, matching OpenOffice Calc.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::DivZero => "#DIV/0!",
            ErrorKind::Value => "#VALUE!",
            ErrorKind::Ref => "#REF!",
            ErrorKind::Name => "#NAME?",
            ErrorKind::NA => "#N/A",
            ErrorKind::Num => "#NUM!",
            ErrorKind::Circular => "Err:522",
            ErrorKind::Syntax => "Err:509",
            ErrorKind::Args => "Err:504",
        }
    }

    /// Parses an error literal.
    pub fn parse(s: &str) -> Option<ErrorKind> {
        Some(match s {
            "#DIV/0!" => ErrorKind::DivZero,
            "#VALUE!" => ErrorKind::Value,
            "#REF!" => ErrorKind::Ref,
            "#NAME?" => ErrorKind::Name,
            "#N/A" => ErrorKind::NA,
            "#NUM!" => ErrorKind::Num,
            "Err:522" => ErrorKind::Circular,
            "Err:509" | "Err:508" | "Err:510" | "Err:511" => ErrorKind::Syntax,
            "Err:504" | "Err:502" => ErrorKind::Args,
            _ => return None,
        })
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The value of a cell or of a formula (sub)expression.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    /// No value.
    #[default]
    Empty,
    /// A number (dates and times are numbers too).
    Number(f64),
    /// Text.
    Text(String),
    /// A logical value.
    Bool(bool),
    /// An error.
    Error(ErrorKind),
}

impl Value {
    /// Short type tag used in the JSON API (`e`, `n`, `s`, `b`, `x`).
    pub fn type_tag(&self) -> &'static str {
        match self {
            Value::Empty => "e",
            Value::Number(_) => "n",
            Value::Text(_) => "s",
            Value::Bool(_) => "b",
            Value::Error(_) => "x",
        }
    }

    /// Whether the value is an error.
    pub fn is_error(&self) -> bool {
        matches!(self, Value::Error(_))
    }

    /// Wraps a number, turning NaN and infinities into `#NUM!`.
    pub fn number(n: f64) -> Value {
        if n.is_finite() {
            Value::Number(n)
        } else {
            Value::Error(ErrorKind::Num)
        }
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::number(n)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Text(s.to_owned())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Text(s)
    }
}

impl From<ErrorKind> for Value {
    fn from(e: ErrorKind) -> Self {
        Value::Error(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_literals_round_trip() {
        for kind in [
            ErrorKind::DivZero, ErrorKind::Value, ErrorKind::Ref, ErrorKind::Name, ErrorKind::NA,
            ErrorKind::Num, ErrorKind::Circular, ErrorKind::Syntax, ErrorKind::Args,
        ] {
            assert_eq!(ErrorKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(ErrorKind::parse("#NOPE"), None);
    }

    #[test]
    fn non_finite_numbers_become_errors() {
        assert_eq!(Value::number(1.5), Value::Number(1.5));
        assert_eq!(Value::number(f64::NAN), Value::Error(ErrorKind::Num));
        assert_eq!(Value::number(f64::INFINITY), Value::Error(ErrorKind::Num));
    }
}
