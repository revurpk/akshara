//! # office-calc
//!
//! The core of Akshara Calc, a spreadsheet compatible with OpenOffice Calc.
//!
//! * [`Workbook`]: sheets of sparse cells with styles, undo/redo, fill, sort
//!   and row/column insertion with reference adjustment;
//! * [`formula`]: the formula language (UI syntax and OpenFormula);
//! * [`eval`]: a pure evaluator with 130+ built-in functions;
//! * incremental recalculation driven by a dependency graph ([`deps`]) with
//!   circular-reference detection, implemented without recursion;
//! * [`load_ods`] / [`save_ods`] and CSV import/export.
//!
//! The crate contains no `unsafe` code. All limits that protect against
//! hostile documents are documented on the items that enforce them.
//!
//! ```
//! use office_calc::{CellAddr, Value, Workbook};
//!
//! let mut wb = Workbook::new();
//! let a = |s: &str| CellAddr::parse(s).unwrap();
//! wb.set_input(0, a("A1"), "19.99").unwrap();
//! wb.set_input(0, a("A2"), "5.01").unwrap();
//! wb.set_input(0, a("A3"), "=SUM(A1:A2)*2").unwrap();
//! assert_eq!(wb.value(0, a("A3")), &Value::Number(50.0));
//!
//! wb.set_input(0, a("A1"), "0").unwrap();          // dependents update
//! assert_eq!(wb.display(0, a("A3")), "10.02");
//! ```

pub mod address;
pub mod csv;
pub mod deps;
pub mod eval;
pub mod format;
pub mod formula;
mod ods;
pub mod value;
mod workbook;

pub use address::{col_name, parse_col, CellAddr, Range, MAX_COLS, MAX_ROWS};
pub use csv::{load_csv, sheet_to_csv};
pub use format::NumberFormat;
pub use ods::{load_ods, save_ods};
pub use value::{ErrorKind, Value};
pub use workbook::{
    is_valid_sheet_name, Axis, Cell, CellStyle, Content, HAlign, PageSetup, PrintArea, Sheet, StylePatch, Workbook, MAX_BULK_CELLS,
    MAX_CELL_TEXT, MAX_SHEETS, MAX_UNDO,
};

use office_odf::{Limits, Result};

/// Loads a workbook from file bytes, detecting the format: ZIP packages are
/// read as `.ods`, everything else as CSV.
pub fn load(bytes: &[u8], limits: &Limits) -> Result<Workbook> {
    if bytes.starts_with(b"PK") {
        load_ods(bytes, limits)
    } else {
        load_csv(bytes, None, limits)
    }
}
