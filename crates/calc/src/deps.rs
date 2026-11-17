//! Dependency graph: which formula cells must be recalculated when a cell
//! changes.
//!
//! Single-cell references are indexed by hash map. Range references are
//! indexed by horizontal *stripes* of [`STRIPE_ROWS`] rows, so finding the
//! ranges that contain a changed cell only inspects ranges near that cell;
//! ranges spanning many stripes go to a short "large ranges" list instead of
//! being registered thousands of times.

use crate::address::{CellAddr, Range};
use crate::formula::{CellRef, Expr};
use std::collections::{HashMap, HashSet};

/// Rows per stripe of the range index.
pub const STRIPE_ROWS: u32 = 256;
/// Ranges covering more stripes than this are kept in the large-range list.
const MAX_STRIPES: u32 = 32;

/// A cell identified across the whole workbook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellKey {
    /// Sheet index.
    pub sheet: u32,
    /// Position on the sheet.
    pub addr: CellAddr,
}

impl CellKey {
    /// Creates a key.
    pub const fn new(sheet: u32, addr: CellAddr) -> Self {
        CellKey { sheet, addr }
    }
}

/// The cells and ranges a formula reads.
#[derive(Debug, Default, PartialEq)]
pub struct Precedents {
    /// Single cells.
    pub cells: Vec<CellKey>,
    /// Multi-cell ranges as `(sheet, range)`.
    pub ranges: Vec<(u32, Range)>,
}

/// Collects the precedents of `expr`, which lives on sheet `home`.
/// References to sheets that do not exist are skipped (they evaluate to
/// `#REF!` and the graph is rebuilt whenever sheets change).
pub fn precedents(expr: &Expr, home: u32, resolve: &dyn Fn(&str) -> Option<usize>) -> Precedents {
    let mut out = Precedents::default();
    expr.visit_refs(&mut |a: &CellRef, b: Option<&CellRef>| {
        let sheet = match &a.sheet {
            None => Some(home),
            Some(name) => resolve(name).map(|i| i as u32),
        };
        let Some(sheet) = sheet else { return };
        match b {
            Some(b) if b.addr != a.addr => out.ranges.push((sheet, Range::new(a.addr, b.addr))),
            _ => out.cells.push(CellKey::new(sheet, a.addr)),
        }
    });
    out
}

/// Reverse dependency index.
#[derive(Debug, Default)]
pub struct DepGraph {
    cells: HashMap<CellKey, Vec<CellKey>>,
    stripes: HashMap<(u32, u32), Vec<(Range, CellKey)>>,
    large: Vec<(u32, Range, CellKey)>,
    /// Formulas that must be recalculated on every pass.
    pub volatile: HashSet<CellKey>,
}

fn stripe_span(range: &Range) -> (u32, u32) {
    (range.start.row / STRIPE_ROWS, range.end.row / STRIPE_ROWS)
}

impl DepGraph {
    /// Removes everything.
    pub fn clear(&mut self) {
        self.cells.clear();
        self.stripes.clear();
        self.large.clear();
        self.volatile.clear();
    }

    /// Records that `formula` reads `precedents`.
    pub fn register(&mut self, formula: CellKey, precedents: &Precedents, volatile: bool) {
        for cell in &precedents.cells {
            let list = self.cells.entry(*cell).or_default();
            if !list.contains(&formula) {
                list.push(formula);
            }
        }
        for (sheet, range) in &precedents.ranges {
            let (first, last) = stripe_span(range);
            if last - first >= MAX_STRIPES {
                self.large.push((*sheet, *range, formula));
            } else {
                for stripe in first..=last {
                    self.stripes.entry((*sheet, stripe)).or_default().push((*range, formula));
                }
            }
        }
        if volatile {
            self.volatile.insert(formula);
        }
    }

    /// Removes the registrations made for `formula` with the same precedents.
    pub fn unregister(&mut self, formula: CellKey, precedents: &Precedents) {
        for cell in &precedents.cells {
            if let Some(list) = self.cells.get_mut(cell) {
                list.retain(|k| *k != formula);
                if list.is_empty() {
                    self.cells.remove(cell);
                }
            }
        }
        for (sheet, range) in &precedents.ranges {
            let (first, last) = stripe_span(range);
            if last - first >= MAX_STRIPES {
                if let Some(pos) = self.large.iter().position(|(s, r, k)| s == sheet && r == range && *k == formula) {
                    self.large.swap_remove(pos);
                }
            } else {
                for stripe in first..=last {
                    if let Some(list) = self.stripes.get_mut(&(*sheet, stripe)) {
                        if let Some(pos) = list.iter().position(|(r, k)| r == range && *k == formula) {
                            list.swap_remove(pos);
                        }
                        if list.is_empty() {
                            self.stripes.remove(&(*sheet, stripe));
                        }
                    }
                }
            }
        }
        self.volatile.remove(&formula);
    }

    /// Calls `f` for every formula that directly reads `cell`. A formula may
    /// be reported more than once.
    pub fn for_each_dependent(&self, cell: CellKey, f: &mut dyn FnMut(CellKey)) {
        if let Some(list) = self.cells.get(&cell) {
            list.iter().copied().for_each(&mut *f);
        }
        if let Some(list) = self.stripes.get(&(cell.sheet, cell.addr.row / STRIPE_ROWS)) {
            for (range, formula) in list {
                if range.contains(cell.addr) {
                    f(*formula);
                }
            }
        }
        for (sheet, range, formula) in &self.large {
            if *sheet == cell.sheet && range.contains(cell.addr) {
                f(*formula);
            }
        }
    }

    /// Number of index entries (for tests and diagnostics).
    pub fn entry_count(&self) -> usize {
        self.cells.values().map(Vec::len).sum::<usize>()
            + self.stripes.values().map(Vec::len).sum::<usize>()
            + self.large.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formula::{parse, Syntax};

    fn key(sheet: u32, a1: &str) -> CellKey {
        CellKey::new(sheet, CellAddr::parse(a1).expect("addr"))
    }

    fn deps_of(graph: &DepGraph, cell: CellKey) -> Vec<CellKey> {
        let mut out = Vec::new();
        graph.for_each_dependent(cell, &mut |k| out.push(k));
        out.sort();
        out.dedup();
        out
    }

    #[test]
    fn collects_precedents() {
        let resolve = |name: &str| (name == "Other").then_some(1usize);
        let expr = parse("A1+SUM(B1:B3)+Other.C1+Missing.D1+SUM(E5:E5)", Syntax::Ui);
        let p = precedents(&expr, 0, &resolve);
        assert_eq!(p.cells, vec![key(0, "A1"), key(1, "C1"), key(0, "E5")]);
        assert_eq!(p.ranges, vec![(0, Range::parse("B1:B3").expect("range"))]);
    }

    #[test]
    fn register_lookup_unregister() {
        let resolve = |_: &str| None;
        let mut graph = DepGraph::default();
        let small = parse("A1+SUM(B1:B300)", Syntax::Ui);
        let big = parse("SUM(C1:C1000000)", Syntax::Ui);
        let p_small = precedents(&small, 0, &resolve);
        let p_big = precedents(&big, 0, &resolve);
        graph.register(key(0, "Z1"), &p_small, false);
        graph.register(key(0, "Z2"), &p_big, true);

        assert_eq!(deps_of(&graph, key(0, "A1")), vec![key(0, "Z1")]);
        assert_eq!(deps_of(&graph, key(0, "B1")), vec![key(0, "Z1")]);
        assert_eq!(deps_of(&graph, key(0, "B300")), vec![key(0, "Z1")], "second stripe");
        assert_eq!(deps_of(&graph, key(0, "B301")), vec![]);
        assert_eq!(deps_of(&graph, key(0, "C500000")), vec![key(0, "Z2")], "large range list");
        assert_eq!(deps_of(&graph, key(1, "A1")), vec![], "other sheet");
        assert!(graph.volatile.contains(&key(0, "Z2")));

        graph.unregister(key(0, "Z1"), &p_small);
        graph.unregister(key(0, "Z2"), &p_big);
        assert_eq!(graph.entry_count(), 0);
        assert!(graph.volatile.is_empty());
        assert_eq!(deps_of(&graph, key(0, "A1")), vec![]);
    }

    #[test]
    fn duplicate_references_register_once() {
        let resolve = |_: &str| None;
        let mut graph = DepGraph::default();
        let expr = parse("A1+A1+A1", Syntax::Ui);
        let p = precedents(&expr, 0, &resolve);
        graph.register(key(0, "B1"), &p, false);
        assert_eq!(graph.entry_count(), 1);
        graph.unregister(key(0, "B1"), &p);
        assert_eq!(graph.entry_count(), 0);
    }
}
