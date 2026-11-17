//! Formula evaluation and the built-in function library.
//!
//! The evaluator is pure: it reads cell values through the [`Host`] trait and
//! has no access to files, the network or the process environment. There are
//! deliberately **no** functions with side effects or external data access
//! (no `DDE`, `WEBSERVICE`, `HYPERLINK`, macros...), so opening a spreadsheet
//! from an untrusted source can never do more than compute numbers.
//!
//! Text results are capped at [`MAX_TEXT_LEN`] so that formulas such as
//! `REPT("x"; 1E9)` or self-doubling concatenations cannot exhaust memory.

use crate::address::{CellAddr, Range};
use crate::format::{self, format_general, parse_number, NumberFormat};
use crate::formula::{BinOp, CellRef, Expr, UnOp};
use crate::value::{ErrorKind, Value};
use std::cmp::Ordering;

/// Longest text a formula may produce, in bytes.
pub const MAX_TEXT_LEN: usize = 1 << 20;

/// Functions whose result changes without any cell changing.
pub const VOLATILE_FUNCTIONS: &[&str] = &["RAND", "RANDBETWEEN", "NOW", "TODAY"];

/// What the evaluator needs from the workbook.
pub trait Host {
    /// Index of the sheet called `name` (case insensitive).
    fn sheet_index(&self, name: &str) -> Option<usize>;
    /// Current value of a cell ([`Value::Empty`] for unused cells).
    fn value(&self, sheet: usize, addr: CellAddr) -> &Value;
    /// Calls `f` for every non-empty cell in `range`, in row-major order.
    fn for_each_in_range(&self, sheet: usize, range: Range, f: &mut dyn FnMut(CellAddr, &Value));
    /// Bottom-right corner of the used area of a sheet, `None` when empty.
    fn extent(&self, sheet: usize) -> Option<CellAddr>;
    /// Current local date and time as a serial number.
    fn now(&self) -> f64;
    /// Next pseudo-random number in `[0, 1)`.
    fn random(&self) -> f64;
}

/// Where a formula is being evaluated.
pub struct Ctx<'a> {
    /// Access to the workbook.
    pub host: &'a dyn Host,
    /// Sheet the formula lives on.
    pub sheet: usize,
    /// Cell the formula lives in.
    pub cell: CellAddr,
}

type R<T> = Result<T, ErrorKind>;

/// Evaluates a formula to the value shown in its cell.
pub fn evaluate(expr: &Expr, ctx: &Ctx) -> Value {
    match eval(expr, ctx) {
        // A formula that refers to an empty cell shows 0, like in Calc.
        Ok(Value::Empty) => Value::Number(0.0),
        Ok(value) => value,
        Err(kind) => Value::Error(kind),
    }
}

enum Operand {
    Scalar(Value),
    Area(usize, Range),
}

fn finite(n: f64) -> R<Value> {
    if n.is_finite() {
        Ok(Value::Number(n))
    } else {
        Err(ErrorKind::Num)
    }
}

fn checked_text(s: String) -> R<Value> {
    if s.len() > MAX_TEXT_LEN {
        Err(ErrorKind::Value)
    } else {
        Ok(Value::Text(s))
    }
}

fn lift(value: &Value) -> R<Value> {
    match value {
        Value::Error(kind) => Err(*kind),
        other => Ok(other.clone()),
    }
}

fn to_number(value: &Value) -> R<f64> {
    match value {
        Value::Number(n) => Ok(*n),
        Value::Bool(b) => Ok(f64::from(u8::from(*b))),
        Value::Empty => Ok(0.0),
        Value::Text(s) => parse_number(s).ok_or(ErrorKind::Value),
        Value::Error(kind) => Err(*kind),
    }
}

fn to_text(value: &Value) -> R<String> {
    match value {
        Value::Text(s) => Ok(s.clone()),
        Value::Number(n) => Ok(format_general(*n)),
        Value::Bool(b) => Ok(if *b { "TRUE" } else { "FALSE" }.into()),
        Value::Empty => Ok(String::new()),
        Value::Error(kind) => Err(*kind),
    }
}

fn to_bool(value: &Value) -> R<bool> {
    match value {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) => Ok(*n != 0.0),
        Value::Empty => Ok(false),
        Value::Text(s) => match s.to_ascii_uppercase().as_str() {
            "TRUE" => Ok(true),
            "FALSE" => Ok(false),
            _ => Err(ErrorKind::Value),
        },
        Value::Error(kind) => Err(*kind),
    }
}

/// Total order used by comparisons, sorting and lookups: numbers (and
/// booleans) sort before text; text compares case-insensitively.
pub fn compare_values(a: &Value, b: &Value) -> Ordering {
    fn numeric(v: &Value) -> Option<f64> {
        match v {
            Value::Number(n) => Some(*n),
            Value::Bool(b) => Some(f64::from(u8::from(*b))),
            _ => None,
        }
    }
    match (a, b) {
        (Value::Empty, Value::Empty) => Ordering::Equal,
        (Value::Text(x), Value::Text(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
        (Value::Empty, Value::Text(y)) => "".cmp(y.as_str()),
        (Value::Text(x), Value::Empty) => x.as_str().cmp(""),
        (Value::Text(_), _) => Ordering::Greater,
        (_, Value::Text(_)) => Ordering::Less,
        _ => {
            let x = numeric(a).unwrap_or(0.0);
            let y = numeric(b).unwrap_or(0.0);
            x.partial_cmp(&y).unwrap_or(Ordering::Equal)
        }
    }
}

fn sheet_of(cell: &CellRef, ctx: &Ctx) -> R<usize> {
    match &cell.sheet {
        None => Ok(ctx.sheet),
        Some(name) => ctx.host.sheet_index(name).ok_or(ErrorKind::Ref),
    }
}

fn operand(expr: &Expr, ctx: &Ctx) -> R<Operand> {
    match expr {
        Expr::Paren(inner) => operand(inner, ctx),
        Expr::Ref(cell) => Ok(Operand::Area(sheet_of(cell, ctx)?, Range::single(cell.addr))),
        Expr::Range(a, b) => Ok(Operand::Area(sheet_of(a, ctx)?, Range::new(a.addr, b.addr))),
        other => Ok(Operand::Scalar(eval(other, ctx)?)),
    }
}

fn area(expr: &Expr, ctx: &Ctx) -> R<(usize, Range)> {
    match operand(expr, ctx)? {
        Operand::Area(sheet, range) => Ok((sheet, range)),
        Operand::Scalar(_) => Err(ErrorKind::Value),
    }
}

/// Restricts a range to the used area of its sheet.
fn clamp(ctx: &Ctx, sheet: usize, range: Range) -> Option<Range> {
    let extent = ctx.host.extent(sheet)?;
    let end = CellAddr::new(range.end.row.min(extent.row), range.end.col.min(extent.col));
    (range.start.row <= end.row && range.start.col <= end.col).then_some(Range { start: range.start, end })
}

/// Evaluates to a scalar. Errors travel in `Err`, so `Ok` never holds
/// [`Value::Error`].
fn eval(expr: &Expr, ctx: &Ctx) -> R<Value> {
    match expr {
        Expr::Number(n) => Ok(Value::Number(*n)),
        Expr::Text(s) => Ok(Value::Text(s.clone())),
        Expr::Bool(b) => Ok(Value::Bool(*b)),
        Expr::Error(kind) => Err(*kind),
        Expr::Missing => Ok(Value::Empty),
        Expr::Name(_) => Err(ErrorKind::Name),
        Expr::Invalid(_) => Err(ErrorKind::Syntax),
        Expr::Paren(inner) => eval(inner, ctx),
        Expr::Ref(cell) => lift(ctx.host.value(sheet_of(cell, ctx)?, cell.addr)),
        Expr::Range(a, b) => {
            // Implicit intersection with the formula's own row or column.
            let sheet = sheet_of(a, ctx)?;
            let range = Range::new(a.addr, b.addr);
            let addr = if range.start == range.end {
                range.start
            } else if range.cols() == 1 && ctx.cell.row >= range.start.row && ctx.cell.row <= range.end.row {
                CellAddr::new(ctx.cell.row, range.start.col)
            } else if range.rows() == 1 && ctx.cell.col >= range.start.col && ctx.cell.col <= range.end.col {
                CellAddr::new(range.start.row, ctx.cell.col)
            } else {
                return Err(ErrorKind::Value);
            };
            lift(ctx.host.value(sheet, addr))
        }
        Expr::Unary(op, inner) => {
            let n = to_number(&eval(inner, ctx)?)?;
            finite(match op {
                UnOp::Neg => -n,
                UnOp::Plus => n,
                UnOp::Percent => n / 100.0,
            })
        }
        Expr::Binary(op, left, right) => {
            let l = eval(left, ctx)?;
            let r = eval(right, ctx)?;
            match op {
                BinOp::Add => finite(to_number(&l)? + to_number(&r)?),
                BinOp::Sub => finite(to_number(&l)? - to_number(&r)?),
                BinOp::Mul => finite(to_number(&l)? * to_number(&r)?),
                BinOp::Div => {
                    let (x, y) = (to_number(&l)?, to_number(&r)?);
                    if y == 0.0 {
                        Err(ErrorKind::DivZero)
                    } else {
                        finite(x / y)
                    }
                }
                BinOp::Pow => finite(to_number(&l)?.powf(to_number(&r)?)),
                BinOp::Concat => {
                    let mut s = to_text(&l)?;
                    let tail = to_text(&r)?;
                    if s.len() + tail.len() > MAX_TEXT_LEN {
                        return Err(ErrorKind::Value);
                    }
                    s.push_str(&tail);
                    Ok(Value::Text(s))
                }
                BinOp::Eq => Ok(Value::Bool(compare_values(&l, &r) == Ordering::Equal)),
                BinOp::Ne => Ok(Value::Bool(compare_values(&l, &r) != Ordering::Equal)),
                BinOp::Lt => Ok(Value::Bool(compare_values(&l, &r) == Ordering::Less)),
                BinOp::Le => Ok(Value::Bool(compare_values(&l, &r) != Ordering::Greater)),
                BinOp::Gt => Ok(Value::Bool(compare_values(&l, &r) == Ordering::Greater)),
                BinOp::Ge => Ok(Value::Bool(compare_values(&l, &r) != Ordering::Less)),
            }
        }
        Expr::Call(name, args) => call(name, args, ctx),
    }
}

fn num(expr: &Expr, ctx: &Ctx) -> R<f64> {
    to_number(&eval(expr, ctx)?)
}

fn text(expr: &Expr, ctx: &Ctx) -> R<String> {
    to_text(&eval(expr, ctx)?)
}

/// Calls `f` with every value an argument stands for: the scalar itself, or
/// every non-empty cell of a range.
fn for_each_value(expr: &Expr, ctx: &Ctx, f: &mut dyn FnMut(&Value, bool) -> R<()>) -> R<()> {
    match operand(expr, ctx) {
        Ok(Operand::Scalar(value)) => f(&value, false),
        Ok(Operand::Area(sheet, range)) => {
            let mut result = Ok(());
            ctx.host.for_each_in_range(sheet, range, &mut |_, value| {
                if result.is_ok() {
                    result = f(value, true);
                }
            });
            result
        }
        Err(kind) => f(&Value::Error(kind), false),
    }
}

/// Collects the numbers of all arguments. In ranges text and blanks are
/// skipped; direct text arguments must be numeric. Errors propagate.
fn numbers(args: &[Expr], ctx: &Ctx) -> R<Vec<f64>> {
    let mut out = Vec::new();
    for arg in args {
        for_each_value(arg, ctx, &mut |value, from_range| {
            match value {
                Value::Number(n) => out.push(*n),
                Value::Bool(b) => out.push(f64::from(u8::from(*b))),
                Value::Error(kind) => return Err(*kind),
                Value::Text(_) if from_range => {}
                Value::Text(s) => out.push(parse_number(s).ok_or(ErrorKind::Value)?),
                Value::Empty => {}
            }
            Ok(())
        })?;
    }
    Ok(out)
}

fn variance(values: &[f64], sample: bool) -> R<f64> {
    let n = values.len() as f64;
    let denom = if sample { n - 1.0 } else { n };
    if denom <= 0.0 {
        return Err(ErrorKind::DivZero);
    }
    let mean = values.iter().sum::<f64>() / n;
    Ok(values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / denom)
}

fn round_half_away(x: f64, digits: f64) -> f64 {
    let p = 10f64.powi(digits.clamp(-300.0, 300.0) as i32);
    let y = x * p;
    // Nudge by a few ULPs so that 1.005 rounds to 1.01 as users expect.
    let r = (y + y.signum() * y.abs() * 4.0 * f64::EPSILON).round();
    r / p
}

fn round_directed(x: f64, digits: f64, up: bool) -> f64 {
    let p = 10f64.powi(digits.clamp(-300.0, 300.0) as i32);
    let y = x * p;
    let near = y.round();
    // Treat values within rounding noise of an integer as that integer.
    let y = if (y - near).abs() < y.abs() * 4.0 * f64::EPSILON { near } else { y };
    let r = if up == (y >= 0.0) { y.ceil() } else { y.floor() };
    r / p
}

fn gcd(a: f64, b: f64) -> f64 {
    let (mut a, mut b) = (a.abs().trunc(), b.abs().trunc());
    while b != 0.0 {
        (a, b) = (b, a % b);
    }
    a
}

// --- criteria (SUMIF, COUNTIF, AVERAGEIF) ----------------------------------

enum CritValue {
    Number(f64),
    Text(String),
}

struct Criterion {
    op: BinOp,
    value: CritValue,
}

fn criterion(value: &Value) -> Criterion {
    match value {
        Value::Text(s) => {
            let (op, rest) = [
                ("<=", BinOp::Le), (">=", BinOp::Ge), ("<>", BinOp::Ne), ("<", BinOp::Lt), (">", BinOp::Gt), ("=", BinOp::Eq),
            ]
            .iter()
            .find_map(|(prefix, op)| s.strip_prefix(prefix).map(|rest| (*op, rest)))
            .unwrap_or((BinOp::Eq, s.as_str()));
            match parse_number(rest) {
                Some(n) => Criterion { op, value: CritValue::Number(n) },
                None => Criterion { op, value: CritValue::Text(rest.to_lowercase()) },
            }
        }
        other => Criterion { op: BinOp::Eq, value: CritValue::Number(to_number(other).unwrap_or(f64::NAN)) },
    }
}

/// Glob match with `*` and `?`, iterative (no exponential backtracking).
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

fn criterion_matches(crit: &Criterion, value: &Value) -> bool {
    match &crit.value {
        CritValue::Number(target) => {
            let cell = match value {
                Value::Number(n) => Some(*n),
                Value::Bool(b) => Some(f64::from(u8::from(*b))),
                Value::Text(s) => parse_number(s),
                _ => None,
            };
            match (cell, crit.op) {
                (Some(n), BinOp::Eq) => n == *target,
                (Some(n), BinOp::Ne) => n != *target,
                (Some(n), BinOp::Lt) => n < *target,
                (Some(n), BinOp::Le) => n <= *target,
                (Some(n), BinOp::Gt) => n > *target,
                (Some(n), BinOp::Ge) => n >= *target,
                (None, BinOp::Ne) => true,
                _ => false,
            }
        }
        CritValue::Text(pattern) => {
            let cell = match value {
                Value::Text(s) => s.to_lowercase(),
                Value::Empty => String::new(),
                Value::Number(n) => format_general(*n),
                Value::Bool(b) => if *b { "true" } else { "false" }.into(),
                Value::Error(_) => return false,
            };
            match crit.op {
                BinOp::Eq => wildcard_match(pattern, &cell),
                BinOp::Ne => !wildcard_match(pattern, &cell),
                BinOp::Lt => cell.as_str() < pattern.as_str(),
                BinOp::Le => cell.as_str() <= pattern.as_str(),
                BinOp::Gt => cell.as_str() > pattern.as_str(),
                BinOp::Ge => cell.as_str() >= pattern.as_str(),
                _ => false,
            }
        }
    }
}

/// Shared implementation of SUMIF / COUNTIF / AVERAGEIF: returns
/// `(sum, count)` of the matching cells.
fn conditional(args: &[Expr], ctx: &Ctx, has_sum_range: bool) -> R<(f64, f64)> {
    let (sheet, range) = area(&args[0], ctx)?;
    let crit = criterion(&eval(&args[1], ctx)?);
    let (sum_sheet, sum_origin) = if has_sum_range && args.len() > 2 {
        let (s, r) = area(&args[2], ctx)?;
        (s, r.start)
    } else {
        (sheet, range.start)
    };
    let mut sum = 0.0;
    let mut count = 0.0;
    let Some(range) = clamp(ctx, sheet, range) else {
        return Ok((0.0, 0.0));
    };
    for row in range.start.row..=range.end.row {
        for col in range.start.col..=range.end.col {
            if !criterion_matches(&crit, ctx.host.value(sheet, CellAddr::new(row, col))) {
                continue;
            }
            count += 1.0;
            let target = CellAddr::new(
                sum_origin.row + (row - range.start.row),
                sum_origin.col + (col - range.start.col),
            );
            if !target.is_valid() {
                continue;
            }
            match ctx.host.value(sum_sheet, target) {
                Value::Number(n) => sum += n,
                Value::Bool(b) => sum += f64::from(u8::from(*b)),
                Value::Error(kind) => return Err(*kind),
                _ => {}
            }
        }
    }
    Ok((sum, count))
}

// --- lookups ----------------------------------------------------------------

fn same_class(a: &Value, b: &Value) -> bool {
    matches!(
        (a, b),
        (Value::Text(_), Value::Text(_)) | (Value::Number(_) | Value::Bool(_), Value::Number(_) | Value::Bool(_))
    )
}

/// Finds `key` in a one-dimensional run of cells. Returns the zero-based
/// position. `mode`: 0 exact, 1 last value <= key (ascending data), -1 last
/// value >= key (descending data).
fn search(ctx: &Ctx, sheet: usize, cells: impl Iterator<Item = CellAddr>, key: &Value, mode: i32) -> Option<usize> {
    let mut best = None;
    for (i, addr) in cells.enumerate() {
        let value = ctx.host.value(sheet, addr);
        if !same_class(value, key) {
            continue;
        }
        let ord = compare_values(value, key);
        match mode {
            0 => {
                let equal = match (value, key) {
                    (Value::Text(v), Value::Text(k)) => wildcard_match(&k.to_lowercase(), &v.to_lowercase()),
                    _ => ord == Ordering::Equal,
                };
                if equal {
                    return Some(i);
                }
            }
            1 => {
                if ord == Ordering::Greater {
                    break;
                }
                best = Some(i);
            }
            _ => {
                if ord == Ordering::Less {
                    break;
                }
                best = Some(i);
            }
        }
    }
    best
}

fn lookup(args: &[Expr], ctx: &Ctx, vertical: bool) -> R<Value> {
    if args.len() < 3 || args.len() > 4 {
        return Err(ErrorKind::Args);
    }
    let key = eval(&args[0], ctx)?;
    let (sheet, range) = area(&args[1], ctx)?;
    let index = num(&args[2], ctx)?.trunc();
    let span = if vertical { range.cols() } else { range.rows() };
    if index < 1.0 || index > f64::from(span) {
        return Err(ErrorKind::Value);
    }
    let offset = index as u32 - 1;
    let sorted = match args.get(3) {
        Some(Expr::Missing) | None => true,
        Some(arg) => to_bool(&eval(arg, ctx)?)?,
    };
    let range = clamp(ctx, sheet, range).ok_or(ErrorKind::NA)?;
    let cells: Box<dyn Iterator<Item = CellAddr>> = if vertical {
        Box::new((range.start.row..=range.end.row).map(move |r| CellAddr::new(r, range.start.col)))
    } else {
        Box::new((range.start.col..=range.end.col).map(move |c| CellAddr::new(range.start.row, c)))
    };
    let pos = search(ctx, sheet, cells, &key, i32::from(sorted)).ok_or(ErrorKind::NA)? as u32;
    let target = if vertical {
        CellAddr::new(range.start.row + pos, range.start.col + offset)
    } else {
        CellAddr::new(range.start.row + offset, range.start.col + pos)
    };
    lift(ctx.host.value(sheet, target))
}

// --- TEXT() formatting --------------------------------------------------------

const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
    "November", "December",
];
const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

fn weekday_index(serial: f64) -> usize {
    ((serial.floor() as i64 + 6).rem_euclid(7)) as usize
}

fn text_format(n: f64, pattern: &str) -> String {
    let lower = pattern.to_ascii_lowercase();
    if lower == "@" || lower == "general" || lower.is_empty() {
        return format_general(n);
    }
    let has_digit_placeholder = lower.contains('0') || lower.contains('#');
    let is_date = !has_digit_placeholder && lower.chars().any(|c| matches!(c, 'y' | 'm' | 'd' | 'h' | 's'));
    if !is_date {
        let percent = lower.contains('%');
        let decimals = lower
            .split_once('.')
            .map(|(_, frac)| frac.chars().take_while(|c| matches!(c, '0' | '#')).count())
            .unwrap_or(0)
            .min(20) as u8;
        if lower.contains("e+") || lower.contains("e-") {
            return format::format_number(n, &NumberFormat::Scientific { decimals });
        }
        let body = format::format_fixed(if percent { n * 100.0 } else { n }, decimals, lower.contains(','));
        return if percent { format!("{body}%") } else { body };
    }
    let Some((year, month, day)) = format::serial_to_date(n) else {
        return format_general(n);
    };
    let (hour, minute, second, _) = format::serial_to_time(n);
    // Tokenise into runs of the same letter.
    let chars: Vec<char> = lower.chars().collect();
    let mut runs: Vec<(char, usize)> = Vec::new();
    for &c in &chars {
        match runs.last_mut() {
            Some((last, count)) if *last == c && c.is_ascii_alphabetic() => *count += 1,
            _ => runs.push((c, 1)),
        }
    }
    let mut out = String::new();
    for (i, &(c, count)) in runs.iter().enumerate() {
        let prev_letter = runs[..i].iter().rev().find(|(ch, _)| ch.is_ascii_alphabetic()).map(|(ch, _)| *ch);
        let next_letter = runs[i + 1..].iter().find(|(ch, _)| ch.is_ascii_alphabetic()).map(|(ch, _)| *ch);
        match c {
            'y' if count <= 2 => out.push_str(&format!("{:02}", year % 100)),
            'y' => out.push_str(&format!("{year:04}")),
            'm' if prev_letter == Some('h') || next_letter == Some('s') => {
                out.push_str(&if count >= 2 { format!("{minute:02}") } else { minute.to_string() });
            }
            'm' => match count {
                1 => out.push_str(&month.to_string()),
                2 => out.push_str(&format!("{month:02}")),
                3 => out.push_str(&MONTHS[month as usize - 1][..3]),
                _ => out.push_str(MONTHS[month as usize - 1]),
            },
            'd' => match count {
                1 => out.push_str(&day.to_string()),
                2 => out.push_str(&format!("{day:02}")),
                3 => out.push_str(&WEEKDAYS[weekday_index(n)][..3]),
                _ => out.push_str(WEEKDAYS[weekday_index(n)]),
            },
            'h' => out.push_str(&if count >= 2 { format!("{hour:02}") } else { hour.to_string() }),
            's' => out.push_str(&if count >= 2 { format!("{second:02}") } else { second.to_string() }),
            other => (0..count).for_each(|_| out.push(other)),
        }
    }
    out
}

fn char_substring(s: &str, start: usize, len: usize) -> String {
    s.chars().skip(start).take(len).collect()
}

/// Normalises `(year, month)` after month arithmetic and returns the serial
/// of the given day, clamped to the end of the month.
fn date_with_month_offset(serial: f64, months: f64, end_of_month: bool) -> R<f64> {
    let (year, month, day) = format::serial_to_date(serial).ok_or(ErrorKind::Num)?;
    let total = year * 12 + i64::from(month) - 1 + months.trunc() as i64;
    let (y, m) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
    let last = format::days_in_month(y, m);
    let d = if end_of_month { last } else { day.min(last) };
    format::date_to_serial(y, m, d).ok_or(ErrorKind::Num)
}

/// Dispatches a function call.
fn call(name: &str, a: &[Expr], ctx: &Ctx) -> R<Value> {
    let arity = |min: usize, max: usize| -> R<()> {
        if a.len() < min || a.len() > max {
            Err(ErrorKind::Args)
        } else {
            Ok(())
        }
    };
    let n = |i: usize| num(&a[i], ctx);
    let n_or = |i: usize, default: f64| -> R<f64> {
        match a.get(i) {
            None | Some(Expr::Missing) => Ok(default),
            Some(arg) => num(arg, ctx),
        }
    };
    let t = |i: usize| text(&a[i], ctx);
    let unary = |f: fn(f64) -> f64| -> R<Value> {
        arity(1, 1)?;
        finite(f(n(0)?))
    };
    let is = |pred: fn(&R<Value>) -> bool| -> R<Value> {
        arity(1, 1)?;
        Ok(Value::Bool(pred(&eval(&a[0], ctx))))
    };

    match name {
        // ---- aggregates ----
        "SUM" => finite(numbers(a, ctx)?.iter().sum()),
        "PRODUCT" => finite(numbers(a, ctx)?.iter().product()),
        "SUMSQ" => finite(numbers(a, ctx)?.iter().map(|x| x * x).sum()),
        "AVERAGE" => {
            let v = numbers(a, ctx)?;
            if v.is_empty() {
                Err(ErrorKind::DivZero)
            } else {
                finite(v.iter().sum::<f64>() / v.len() as f64)
            }
        }
        "GEOMEAN" => {
            let v = numbers(a, ctx)?;
            if v.is_empty() || v.iter().any(|x| *x <= 0.0) {
                return Err(ErrorKind::Num);
            }
            finite((v.iter().map(|x| x.ln()).sum::<f64>() / v.len() as f64).exp())
        }
        "MIN" => finite(numbers(a, ctx)?.into_iter().reduce(f64::min).unwrap_or(0.0)),
        "MAX" => finite(numbers(a, ctx)?.into_iter().reduce(f64::max).unwrap_or(0.0)),
        "COUNT" | "COUNTA" => {
            let mut count = 0.0;
            for arg in a {
                for_each_value(arg, ctx, &mut |value, _| {
                    let hit = match value {
                        Value::Number(_) | Value::Bool(_) => true,
                        Value::Empty => false,
                        _ => name == "COUNTA",
                    };
                    count += f64::from(u8::from(hit));
                    Ok(())
                })?;
            }
            Ok(Value::Number(count))
        }
        "COUNTBLANK" => {
            arity(1, 1)?;
            let (sheet, range) = area(&a[0], ctx)?;
            let mut used = 0u64;
            ctx.host.for_each_in_range(sheet, range, &mut |_, value| {
                if !matches!(value, Value::Empty) && !matches!(value, Value::Text(s) if s.is_empty()) {
                    used += 1;
                }
            });
            Ok(Value::Number((range.cell_count() - used) as f64))
        }
        "MEDIAN" => {
            let mut v = numbers(a, ctx)?;
            if v.is_empty() {
                return Err(ErrorKind::Num);
            }
            v.sort_by(|x, y| x.partial_cmp(y).unwrap_or(Ordering::Equal));
            let mid = v.len() / 2;
            finite(if v.len() % 2 == 1 { v[mid] } else { (v[mid - 1] + v[mid]) / 2.0 })
        }
        "MODE" => {
            let mut v = numbers(a, ctx)?;
            v.sort_by(|x, y| x.partial_cmp(y).unwrap_or(Ordering::Equal));
            let mut best: Option<(f64, usize)> = None;
            let mut i = 0;
            while i < v.len() {
                let mut j = i;
                while j < v.len() && v[j] == v[i] {
                    j += 1;
                }
                if j - i > 1 && best.is_none_or(|(_, count)| j - i > count) {
                    best = Some((v[i], j - i));
                }
                i = j;
            }
            best.map(|(value, _)| Value::Number(value)).ok_or(ErrorKind::Value)
        }
        "VAR" => finite(variance(&numbers(a, ctx)?, true)?),
        "VARP" => finite(variance(&numbers(a, ctx)?, false)?),
        "STDEV" => finite(variance(&numbers(a, ctx)?, true)?.sqrt()),
        "STDEVP" => finite(variance(&numbers(a, ctx)?, false)?.sqrt()),
        "LARGE" | "SMALL" => {
            arity(2, 2)?;
            let mut v = numbers(&a[..1], ctx)?;
            let k = n(1)?.trunc();
            if k < 1.0 || k > v.len() as f64 {
                return Err(ErrorKind::Value);
            }
            v.sort_by(|x, y| x.partial_cmp(y).unwrap_or(Ordering::Equal));
            if name == "LARGE" {
                v.reverse();
            }
            Ok(Value::Number(v[k as usize - 1]))
        }
        "RANK" => {
            arity(2, 3)?;
            let x = n(0)?;
            let v = numbers(&a[1..2], ctx)?;
            let ascending = n_or(2, 0.0)? != 0.0;
            if !v.contains(&x) {
                return Err(ErrorKind::NA);
            }
            let before = v.iter().filter(|y| if ascending { **y < x } else { **y > x }).count();
            Ok(Value::Number(before as f64 + 1.0))
        }
        "SUMPRODUCT" => {
            arity(1, 30)?;
            let areas: Vec<(usize, Range)> = a.iter().map(|arg| area(arg, ctx)).collect::<R<_>>()?;
            let first = areas[0].1;
            if areas.iter().any(|(_, r)| r.rows() != first.rows() || r.cols() != first.cols()) {
                return Err(ErrorKind::Value);
            }
            let Some(clamped) = clamp(ctx, areas[0].0, first) else {
                return Ok(Value::Number(0.0));
            };
            let mut total = 0.0;
            for dr in 0..clamped.rows() {
                for dc in 0..clamped.cols() {
                    let mut product = 1.0;
                    for (sheet, range) in &areas {
                        let addr = CellAddr::new(range.start.row + dr, range.start.col + dc);
                        product *= match ctx.host.value(*sheet, addr) {
                            Value::Number(x) => *x,
                            Value::Bool(b) => f64::from(u8::from(*b)),
                            Value::Error(kind) => return Err(*kind),
                            _ => 0.0,
                        };
                    }
                    total += product;
                }
            }
            finite(total)
        }
        "SUMIF" => {
            arity(2, 3)?;
            finite(conditional(a, ctx, true)?.0)
        }
        "COUNTIF" => {
            arity(2, 2)?;
            Ok(Value::Number(conditional(a, ctx, false)?.1))
        }
        "AVERAGEIF" => {
            arity(2, 3)?;
            let (sum, count) = conditional(a, ctx, true)?;
            if count == 0.0 {
                Err(ErrorKind::DivZero)
            } else {
                finite(sum / count)
            }
        }

        // ---- maths ----
        "ABS" => unary(f64::abs),
        "SIGN" => unary(|x| if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { 0.0 }),
        "SQRT" => unary(f64::sqrt),
        "EXP" => unary(f64::exp),
        "LN" => unary(f64::ln),
        "LOG10" => unary(f64::log10),
        "SIN" => unary(f64::sin),
        "COS" => unary(f64::cos),
        "TAN" => unary(f64::tan),
        "ASIN" => unary(f64::asin),
        "ACOS" => unary(f64::acos),
        "ATAN" => unary(f64::atan),
        "SINH" => unary(f64::sinh),
        "COSH" => unary(f64::cosh),
        "TANH" => unary(f64::tanh),
        "DEGREES" => unary(f64::to_degrees),
        "RADIANS" => unary(f64::to_radians),
        "INT" => unary(f64::floor),
        "EVEN" => unary(|x| {
            let r = (x.abs() / 2.0).ceil() * 2.0;
            if x < 0.0 { -r } else { r }
        }),
        "ODD" => unary(|x| {
            let r = ((x.abs() - 1.0) / 2.0).ceil().max(0.0) * 2.0 + 1.0;
            if x < 0.0 { -r } else { r }
        }),
        "FACT" => {
            arity(1, 1)?;
            let x = n(0)?.trunc();
            if !(0.0..=170.0).contains(&x) {
                return Err(ErrorKind::Num);
            }
            finite((1..=x as u32).map(f64::from).product())
        }
        "LOG" => {
            arity(1, 2)?;
            finite(n(0)?.log(n_or(1, 10.0)?))
        }
        "POWER" => {
            arity(2, 2)?;
            finite(n(0)?.powf(n(1)?))
        }
        "ATAN2" => {
            arity(2, 2)?;
            finite(n(1)?.atan2(n(0)?))
        }
        "MOD" => {
            arity(2, 2)?;
            let (x, y) = (n(0)?, n(1)?);
            if y == 0.0 {
                return Err(ErrorKind::DivZero);
            }
            finite(x - y * (x / y).floor())
        }
        "QUOTIENT" => {
            arity(2, 2)?;
            let (x, y) = (n(0)?, n(1)?);
            if y == 0.0 {
                return Err(ErrorKind::DivZero);
            }
            finite((x / y).trunc())
        }
        "ROUND" => {
            arity(1, 2)?;
            finite(round_half_away(n(0)?, n_or(1, 0.0)?.trunc()))
        }
        "ROUNDUP" => {
            arity(1, 2)?;
            finite(round_directed(n(0)?, n_or(1, 0.0)?.trunc(), true))
        }
        "ROUNDDOWN" | "TRUNC" => {
            arity(1, 2)?;
            finite(round_directed(n(0)?, n_or(1, 0.0)?.trunc(), false))
        }
        "CEILING" | "FLOOR" => {
            arity(1, 2)?;
            let x = n(0)?;
            let step = n_or(1, if x < 0.0 { -1.0 } else { 1.0 })?;
            if step == 0.0 {
                return Ok(Value::Number(0.0));
            }
            let q = x / step;
            finite(if name == "CEILING" { q.ceil() } else { q.floor() } * step)
        }
        "GCD" | "LCM" => {
            let v = numbers(a, ctx)?;
            if v.is_empty() || v.iter().any(|x| *x < 0.0) {
                return Err(ErrorKind::Num);
            }
            let fold = |acc: f64, x: f64| -> f64 {
                if name == "GCD" {
                    gcd(acc, x)
                } else if acc == 0.0 || x.trunc() == 0.0 {
                    0.0
                } else {
                    acc / gcd(acc, x) * x.trunc()
                }
            };
            finite(v[1..].iter().fold(v[0].trunc(), |acc, x| fold(acc, *x)))
        }
        "COMBIN" => {
            arity(2, 2)?;
            let (total, k) = (n(0)?.trunc(), n(1)?.trunc());
            if total < 0.0 || k < 0.0 || k > total {
                return Err(ErrorKind::Num);
            }
            let k = k.min(total - k);
            let mut result = 1.0;
            let mut i = 0.0;
            while i < k {
                result = result * (total - i) / (i + 1.0);
                i += 1.0;
            }
            finite(result.round())
        }
        "PI" => {
            arity(0, 0)?;
            Ok(Value::Number(std::f64::consts::PI))
        }
        "RAND" => {
            arity(0, 0)?;
            Ok(Value::Number(ctx.host.random()))
        }
        "RANDBETWEEN" => {
            arity(2, 2)?;
            let (lo, hi) = (n(0)?.ceil(), n(1)?.floor());
            if lo > hi {
                return Err(ErrorKind::Value);
            }
            finite(lo + (ctx.host.random() * (hi - lo + 1.0)).floor())
        }

        // ---- logic ----
        "IF" => {
            arity(1, 3)?;
            let branch = if to_bool(&eval(&a[0], ctx)?)? { a.get(1) } else { a.get(2) };
            match branch {
                Some(expr) => eval(expr, ctx),
                None => Ok(Value::Bool(a.len() < 2 && to_bool(&eval(&a[0], ctx)?)?)),
            }
        }
        "AND" | "OR" | "XOR" => {
            arity(1, 255)?;
            let mut trues = 0usize;
            let mut total = 0usize;
            for arg in a {
                for_each_value(arg, ctx, &mut |value, from_range| {
                    match value {
                        Value::Text(_) | Value::Empty if from_range => {}
                        other => {
                            total += 1;
                            trues += usize::from(to_bool(other)?);
                        }
                    }
                    Ok(())
                })?;
            }
            if total == 0 {
                return Err(ErrorKind::Value);
            }
            Ok(Value::Bool(match name {
                "AND" => trues == total,
                "OR" => trues > 0,
                _ => trues % 2 == 1,
            }))
        }
        "NOT" => {
            arity(1, 1)?;
            Ok(Value::Bool(!to_bool(&eval(&a[0], ctx)?)?))
        }
        "TRUE" => arity(0, 0).map(|()| Value::Bool(true)),
        "FALSE" => arity(0, 0).map(|()| Value::Bool(false)),
        "NA" => arity(0, 0).and(Err(ErrorKind::NA)),
        "IFERROR" => {
            arity(2, 2)?;
            eval(&a[0], ctx).or_else(|_| eval(&a[1], ctx))
        }
        "IFNA" => {
            arity(2, 2)?;
            match eval(&a[0], ctx) {
                Err(ErrorKind::NA) => eval(&a[1], ctx),
                other => other,
            }
        }
        "ISBLANK" => is(|v| matches!(v, Ok(Value::Empty))),
        "ISNUMBER" => is(|v| matches!(v, Ok(Value::Number(_)))),
        "ISTEXT" => is(|v| matches!(v, Ok(Value::Text(_)))),
        "ISNONTEXT" => is(|v| !matches!(v, Ok(Value::Text(_)))),
        "ISLOGICAL" => is(|v| matches!(v, Ok(Value::Bool(_)))),
        "ISERROR" => is(|v| v.is_err()),
        "ISERR" => is(|v| matches!(v, Err(kind) if *kind != ErrorKind::NA)),
        "ISNA" => is(|v| matches!(v, Err(ErrorKind::NA))),
        "ISEVEN" | "ISODD" => {
            arity(1, 1)?;
            let even = n(0)?.trunc() % 2.0 == 0.0;
            Ok(Value::Bool(even == (name == "ISEVEN")))
        }
        "CHOOSE" => {
            arity(2, 255)?;
            let index = n(0)?.trunc();
            if index < 1.0 || index >= a.len() as f64 {
                return Err(ErrorKind::Value);
            }
            eval(&a[index as usize], ctx)
        }

        // ---- text ----
        "CONCATENATE" | "CONCAT" => {
            let mut out = String::new();
            for arg in a {
                for_each_value(arg, ctx, &mut |value, _| {
                    out.push_str(&to_text(value)?);
                    if out.len() > MAX_TEXT_LEN {
                        return Err(ErrorKind::Value);
                    }
                    Ok(())
                })?;
            }
            Ok(Value::Text(out))
        }
        "TEXTJOIN" => {
            arity(3, 255)?;
            let delimiter = t(0)?;
            let skip_empty = to_bool(&eval(&a[1], ctx)?)?;
            let mut out = String::new();
            let mut first = true;
            for arg in &a[2..] {
                for_each_value(arg, ctx, &mut |value, _| {
                    let piece = to_text(value)?;
                    if piece.is_empty() && skip_empty {
                        return Ok(());
                    }
                    if !first {
                        out.push_str(&delimiter);
                    }
                    first = false;
                    out.push_str(&piece);
                    if out.len() > MAX_TEXT_LEN {
                        return Err(ErrorKind::Value);
                    }
                    Ok(())
                })?;
            }
            Ok(Value::Text(out))
        }
        "LEN" => {
            arity(1, 1)?;
            Ok(Value::Number(t(0)?.chars().count() as f64))
        }
        "LEFT" | "RIGHT" => {
            arity(1, 2)?;
            let s = t(0)?;
            let count = n_or(1, 1.0)?;
            if count < 0.0 {
                return Err(ErrorKind::Value);
            }
            let count = count.min(MAX_TEXT_LEN as f64) as usize;
            let total = s.chars().count();
            let skip = if name == "LEFT" { 0 } else { total.saturating_sub(count) };
            Ok(Value::Text(char_substring(&s, skip, count)))
        }
        "MID" => {
            arity(3, 3)?;
            let s = t(0)?;
            let (start, len) = (n(1)?, n(2)?);
            if start < 1.0 || len < 0.0 {
                return Err(ErrorKind::Value);
            }
            let start = start.min(MAX_TEXT_LEN as f64 + 1.0) as usize - 1;
            Ok(Value::Text(char_substring(&s, start, len.min(MAX_TEXT_LEN as f64) as usize)))
        }
        "UPPER" => arity(1, 1).and_then(|()| Ok(Value::Text(t(0)?.to_uppercase()))),
        "LOWER" => arity(1, 1).and_then(|()| Ok(Value::Text(t(0)?.to_lowercase()))),
        "PROPER" => {
            arity(1, 1)?;
            let mut out = String::new();
            let mut start_of_word = true;
            for c in t(0)?.chars() {
                if start_of_word {
                    out.extend(c.to_uppercase());
                } else {
                    out.extend(c.to_lowercase());
                }
                start_of_word = !c.is_alphanumeric();
            }
            Ok(Value::Text(out))
        }
        "TRIM" => {
            arity(1, 1)?;
            Ok(Value::Text(t(0)?.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")))
        }
        "SUBSTITUTE" => {
            arity(3, 4)?;
            let (s, old, new) = (t(0)?, t(1)?, t(2)?);
            if old.is_empty() {
                return Ok(Value::Text(s));
            }
            let occurrences = s.matches(old.as_str()).count();
            match a.get(3) {
                None | Some(Expr::Missing) => {
                    let grown = s.len() + occurrences * new.len().saturating_sub(old.len());
                    if grown > MAX_TEXT_LEN {
                        return Err(ErrorKind::Value);
                    }
                    Ok(Value::Text(s.replace(old.as_str(), &new)))
                }
                Some(arg) => {
                    let nth = num(arg, ctx)?.trunc();
                    if nth < 1.0 {
                        return Err(ErrorKind::Value);
                    }
                    match s.match_indices(old.as_str()).nth(nth as usize - 1) {
                        Some((idx, _)) => {
                            checked_text(format!("{}{}{}", &s[..idx], new, &s[idx + old.len()..]))
                        }
                        None => Ok(Value::Text(s)),
                    }
                }
            }
        }
        "REPLACE" => {
            arity(4, 4)?;
            let s = t(0)?;
            let (start, len) = (n(1)?, n(2)?);
            if start < 1.0 || len < 0.0 {
                return Err(ErrorKind::Value);
            }
            let start = start.min(MAX_TEXT_LEN as f64 + 1.0) as usize - 1;
            let len = len.min(MAX_TEXT_LEN as f64) as usize;
            let head: String = s.chars().take(start).collect();
            let tail: String = s.chars().skip(start.saturating_add(len)).collect();
            checked_text(format!("{head}{}{tail}", t(3)?))
        }
        "FIND" | "SEARCH" => {
            arity(2, 3)?;
            let (mut needle, mut hay) = (t(0)?, t(1)?);
            if name == "SEARCH" {
                needle = needle.to_lowercase();
                hay = hay.to_lowercase();
            }
            let start = n_or(2, 1.0)?;
            if start < 1.0 {
                return Err(ErrorKind::Value);
            }
            let skip = start as usize - 1;
            let byte_start = hay.char_indices().nth(skip).map(|(i, _)| i).unwrap_or(hay.len());
            if skip > hay.chars().count() {
                return Err(ErrorKind::Value);
            }
            match hay[byte_start..].find(needle.as_str()) {
                Some(idx) => Ok(Value::Number((hay[..byte_start + idx].chars().count() + 1) as f64)),
                None => Err(ErrorKind::Value),
            }
        }
        "REPT" => {
            arity(2, 2)?;
            let s = t(0)?;
            let count = n(1)?;
            if count < 0.0 {
                return Err(ErrorKind::Value);
            }
            if s.len() as f64 * count.trunc() > MAX_TEXT_LEN as f64 {
                return Err(ErrorKind::Value);
            }
            Ok(Value::Text(s.repeat(count as usize)))
        }
        "EXACT" => {
            arity(2, 2)?;
            Ok(Value::Bool(t(0)? == t(1)?))
        }
        "CHAR" => {
            arity(1, 1)?;
            let code = n(0)?.trunc();
            if !(1.0..=1_114_111.0).contains(&code) {
                return Err(ErrorKind::Value);
            }
            char::from_u32(code as u32).map(|c| Value::Text(c.to_string())).ok_or(ErrorKind::Value)
        }
        "CODE" => {
            arity(1, 1)?;
            t(0)?.chars().next().map(|c| Value::Number(f64::from(u32::from(c)))).ok_or(ErrorKind::Value)
        }
        "VALUE" => {
            arity(1, 1)?;
            match eval(&a[0], ctx)? {
                Value::Text(s) => match format::parse_input(&s) {
                    format::Input::Number(x, _) => Ok(Value::Number(x)),
                    _ => Err(ErrorKind::Value),
                },
                other => finite(to_number(&other)?),
            }
        }
        "T" => {
            arity(1, 1)?;
            Ok(match eval(&a[0], ctx)? {
                Value::Text(s) => Value::Text(s),
                _ => Value::Text(String::new()),
            })
        }
        "N" => {
            arity(1, 1)?;
            Ok(Value::Number(match eval(&a[0], ctx)? {
                Value::Number(x) => x,
                Value::Bool(b) => f64::from(u8::from(b)),
                _ => 0.0,
            }))
        }
        "TEXT" => {
            arity(2, 2)?;
            let value = eval(&a[0], ctx)?;
            let pattern = t(1)?;
            match value {
                Value::Text(s) => Ok(Value::Text(s)),
                other => checked_text(text_format(to_number(&other)?, &pattern)),
            }
        }

        // ---- lookup ----
        "VLOOKUP" => lookup(a, ctx, true),
        "HLOOKUP" => lookup(a, ctx, false),
        "MATCH" => {
            arity(2, 3)?;
            let key = eval(&a[0], ctx)?;
            let (sheet, range) = area(&a[1], ctx)?;
            let mode = n_or(2, 1.0)?;
            let mode = if mode > 0.0 { 1 } else if mode < 0.0 { -1 } else { 0 };
            if range.rows() != 1 && range.cols() != 1 {
                return Err(ErrorKind::NA);
            }
            let range = clamp(ctx, sheet, range).ok_or(ErrorKind::NA)?;
            let cells: Box<dyn Iterator<Item = CellAddr>> = if range.cols() == 1 {
                Box::new((range.start.row..=range.end.row).map(move |r| CellAddr::new(r, range.start.col)))
            } else {
                Box::new((range.start.col..=range.end.col).map(move |c| CellAddr::new(range.start.row, c)))
            };
            search(ctx, sheet, cells, &key, mode).map(|i| Value::Number(i as f64 + 1.0)).ok_or(ErrorKind::NA)
        }
        "INDEX" => {
            arity(2, 3)?;
            let (sheet, range) = area(&a[0], ctx)?;
            let first = n(1)?.trunc();
            let (row, col) = if a.len() == 2 && range.rows() == 1 {
                (1.0, first)
            } else {
                (first, n_or(2, 1.0)?.trunc())
            };
            let row = if row == 0.0 && range.rows() == 1 { 1.0 } else { row };
            let col = if col == 0.0 && range.cols() == 1 { 1.0 } else { col };
            if row < 1.0 || col < 1.0 || row > f64::from(range.rows()) || col > f64::from(range.cols()) {
                return Err(ErrorKind::Ref);
            }
            lift(ctx.host.value(
                sheet,
                CellAddr::new(range.start.row + row as u32 - 1, range.start.col + col as u32 - 1),
            ))
        }
        "ROW" | "COLUMN" => {
            arity(0, 1)?;
            let addr = match a.first() {
                None | Some(Expr::Missing) => ctx.cell,
                Some(arg) => area(arg, ctx)?.1.start,
            };
            Ok(Value::Number(f64::from(if name == "ROW" { addr.row } else { addr.col }) + 1.0))
        }
        "ROWS" | "COLUMNS" => {
            arity(1, 1)?;
            let range = area(&a[0], ctx)?.1;
            Ok(Value::Number(f64::from(if name == "ROWS" { range.rows() } else { range.cols() })))
        }

        // ---- date and time ----
        "DATE" => {
            arity(3, 3)?;
            let (y, m, d) = (n(0)?.trunc(), n(1)?.trunc(), n(2)?.trunc());
            if !(0.0..=9999.0).contains(&y) || m.abs() > 120_000.0 || d.abs() > 4_000_000.0 {
                return Err(ErrorKind::Num);
            }
            let total = y as i64 * 12 + m as i64 - 1;
            let first = format::date_to_serial(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, 1)
                .ok_or(ErrorKind::Num)?;
            let serial = first + d - 1.0;
            format::serial_to_date(serial).map(|_| Value::Number(serial)).ok_or(ErrorKind::Num)
        }
        "TIME" => {
            arity(3, 3)?;
            finite((n(0)? * 3600.0 + n(1)? * 60.0 + n(2)?) / 86_400.0)
        }
        "TODAY" => arity(0, 0).map(|()| Value::Number(ctx.host.now().floor())),
        "NOW" => arity(0, 0).map(|()| Value::Number(ctx.host.now())),
        "YEAR" | "MONTH" | "DAY" => {
            arity(1, 1)?;
            let (y, m, d) = format::serial_to_date(n(0)?).ok_or(ErrorKind::Num)?;
            Ok(Value::Number(match name {
                "YEAR" => y as f64,
                "MONTH" => f64::from(m),
                _ => f64::from(d),
            }))
        }
        "HOUR" | "MINUTE" | "SECOND" => {
            arity(1, 1)?;
            let (h, m, s, _) = format::serial_to_time(n(0)?);
            Ok(Value::Number(f64::from(match name {
                "HOUR" => h,
                "MINUTE" => m,
                _ => s,
            })))
        }
        "WEEKDAY" => {
            arity(1, 2)?;
            let serial = n(0)?;
            format::serial_to_date(serial).ok_or(ErrorKind::Num)?;
            let sunday_based = weekday_index(serial) as f64; // 0 = Sunday
            Ok(Value::Number(match n_or(1, 1.0)?.trunc() as i64 {
                1 => sunday_based + 1.0,
                2 => (sunday_based + 6.0) % 7.0 + 1.0,
                3 => (sunday_based + 6.0) % 7.0,
                _ => return Err(ErrorKind::Value),
            }))
        }
        "DAYS" => {
            arity(2, 2)?;
            finite(n(0)?.floor() - n(1)?.floor())
        }
        "EDATE" => {
            arity(2, 2)?;
            finite(date_with_month_offset(n(0)?, n(1)?, false)?)
        }
        "EOMONTH" => {
            arity(2, 2)?;
            finite(date_with_month_offset(n(0)?, n(1)?, true)?)
        }

        // ---- financial ----
        "PMT" => {
            arity(3, 5)?;
            let (rate, nper, pv) = (n(0)?, n(1)?, n(2)?);
            let (fv, due) = (n_or(3, 0.0)?, n_or(4, 0.0)?);
            if nper == 0.0 {
                return Err(ErrorKind::DivZero);
            }
            if rate == 0.0 {
                return finite(-(pv + fv) / nper);
            }
            let growth = (1.0 + rate).powf(nper);
            let due = if due != 0.0 { 1.0 } else { 0.0 };
            finite(-(rate * (pv * growth + fv)) / ((1.0 + rate * due) * (growth - 1.0)))
        }
        "FV" => {
            arity(3, 5)?;
            let (rate, nper, pmt) = (n(0)?, n(1)?, n(2)?);
            let (pv, due) = (n_or(3, 0.0)?, n_or(4, 0.0)?);
            if rate == 0.0 {
                return finite(-(pv + pmt * nper));
            }
            let growth = (1.0 + rate).powf(nper);
            let due = if due != 0.0 { 1.0 } else { 0.0 };
            finite(-(pv * growth + pmt * (1.0 + rate * due) * (growth - 1.0) / rate))
        }
        "PV" => {
            arity(3, 5)?;
            let (rate, nper, pmt) = (n(0)?, n(1)?, n(2)?);
            let (fv, due) = (n_or(3, 0.0)?, n_or(4, 0.0)?);
            if rate == 0.0 {
                return finite(-(fv + pmt * nper));
            }
            let growth = (1.0 + rate).powf(nper);
            let due = if due != 0.0 { 1.0 } else { 0.0 };
            finite(-(fv + pmt * (1.0 + rate * due) * (growth - 1.0) / rate) / growth)
        }
        "NPV" => {
            arity(2, 255)?;
            let rate = n(0)?;
            let values = numbers(&a[1..], ctx)?;
            if rate == -1.0 {
                return Err(ErrorKind::DivZero);
            }
            finite(values.iter().enumerate().map(|(i, v)| v / (1.0 + rate).powi(i as i32 + 1)).sum())
        }

        _ => Err(ErrorKind::Name),
    }
}

/// Names of all built-in functions, sorted. Used by the UI for completion and
/// by the documentation tests.
pub const FUNCTION_NAMES: &[&str] = &[
    "ABS", "ACOS", "AND", "ASIN", "ATAN", "ATAN2", "AVERAGE", "AVERAGEIF", "CEILING", "CHAR", "CHOOSE",
    "CODE", "COLUMN", "COLUMNS", "COMBIN", "CONCAT", "CONCATENATE", "COS", "COSH", "COUNT", "COUNTA",
    "COUNTBLANK", "COUNTIF", "DATE", "DAY", "DAYS", "DEGREES", "EDATE", "EOMONTH", "EVEN", "EXACT", "EXP",
    "FACT", "FALSE", "FIND", "FLOOR", "FV", "GCD", "GEOMEAN", "HLOOKUP", "HOUR", "IF", "IFERROR", "IFNA",
    "INDEX", "INT", "ISBLANK", "ISERR", "ISERROR", "ISEVEN", "ISLOGICAL", "ISNA", "ISNONTEXT", "ISNUMBER",
    "ISODD", "ISTEXT", "LARGE", "LCM", "LEFT", "LEN", "LN", "LOG", "LOG10", "LOWER", "MATCH", "MAX",
    "MEDIAN", "MID", "MIN", "MINUTE", "MOD", "MODE", "MONTH", "N", "NA", "NOT", "NOW", "NPV", "ODD", "OR",
    "PI", "PMT", "POWER", "PRODUCT", "PROPER", "PV", "QUOTIENT", "RADIANS", "RAND", "RANDBETWEEN", "RANK",
    "REPLACE", "REPT", "RIGHT", "ROUND", "ROUNDDOWN", "ROUNDUP", "ROW", "ROWS", "SEARCH", "SECOND", "SIGN",
    "SIN", "SINH", "SMALL", "SQRT", "STDEV", "STDEVP", "SUBSTITUTE", "SUM", "SUMIF", "SUMPRODUCT", "SUMSQ",
    "T", "TAN", "TANH", "TEXT", "TEXTJOIN", "TIME", "TODAY", "TRIM", "TRUE", "TRUNC", "UPPER", "VALUE",
    "VAR", "VARP", "VLOOKUP", "WEEKDAY", "XOR", "YEAR",
];
