//! Behavioural tests of the spreadsheet engine: functions, recalculation,
//! circular references, structural edits and undo.

use office_calc::{Axis, CellAddr, Content, ErrorKind, HAlign, NumberFormat, Range, StylePatch, Value, Workbook};

fn a(s: &str) -> CellAddr {
    CellAddr::parse(s).unwrap_or_else(|| panic!("bad address {s}"))
}

fn r(s: &str) -> Range {
    Range::parse(s).unwrap_or_else(|| panic!("bad range {s}"))
}

/// Builds a workbook from `(address, input)` pairs on the first sheet.
fn wb(cells: &[(&str, &str)]) -> Workbook {
    let mut wb = Workbook::new();
    wb.set_fixed_now(Some(45_351.75)); // 2024-02-29 18:00
    for (addr, input) in cells {
        wb.set_input(0, a(addr), input).expect("set_input");
    }
    wb
}

/// Evaluates one formula against a small fixed data set.
fn calc(formula: &str) -> String {
    let mut book = wb(&[
        ("A1", "1"), ("A2", "2"), ("A3", "3"), ("A4", "4"), ("A5", "text"), ("A6", "TRUE"),
        ("B1", "apple"), ("B2", "Banana"), ("B3", "cherry"), ("B4", "apple"),
        ("C1", "10"), ("C2", "20"), ("C3", "30"), ("C4", "40"),
        ("D1", "=1/0"), ("D2", "=NA()"),
        ("E1", "2024-02-29"), ("E2", "0.5"),
    ]);
    book.set_input(0, a("Z1"), formula).expect("set formula");
    book.display(0, a("Z1"))
}

#[track_caller]
fn check(formula: &str, expected: &str) {
    assert_eq!(calc(formula), expected, "{formula}");
}

#[test]
fn arithmetic_and_operators() {
    check("=1+2*3", "7");
    check("=(1+2)*3", "9");
    check("=2^10", "1024");
    check("=-2^2", "4");
    check("=2^3^2", "64");
    check("=10/4", "2.5");
    check("=1/0", "#DIV/0!");
    check("=50%", "0.5");
    check("=\"a\"&\"b\"&1", "ab1");
    check("=1=1", "TRUE");
    check("=1<>1", "FALSE");
    check("=\"a\"<\"B\"", "TRUE");
    check("=\"abc\"=\"ABC\"", "TRUE");
    check("=1<\"a\"", "TRUE");
    check("=\"5\"+1", "6");
    check("=\"x\"+1", "#VALUE!");
    check("=A1+A2", "3");
    check("=A5+1", "#VALUE!");
    check("=A6+1", "2");
    check("=Z99", "0");
    check("=Z99&\"x\"", "x");
    check("=0.1+0.2", "0.3");
    check("=1e308*10", "#NUM!");
    check("=D1+1", "#DIV/0!");
    check("=+A1", "1");
}

#[test]
fn aggregate_functions() {
    check("=SUM(A1:A4)", "10");
    check("=SUM(A1:A6)", "11");
    check("=SUM(A1:A4;10;\"5\")", "25");
    check("=SUM(\"x\")", "#VALUE!");
    check("=SUM(D1:D2)", "#DIV/0!");
    check("=SUM()", "0");
    check("=PRODUCT(A1:A4)", "24");
    check("=AVERAGE(A1:A4)", "2.5");
    check("=AVERAGE(B1:B4)", "#DIV/0!");
    check("=COUNT(A1:A6)", "5");
    check("=COUNTA(A1:A6)", "6");
    check("=COUNTA(D1:D2)", "2");
    check("=COUNTBLANK(A1:A10)", "4");
    check("=MIN(A1:A4)", "1");
    check("=MAX(A1:A4;99)", "99");
    check("=MEDIAN(A1:A4)", "2.5");
    check("=MEDIAN(1;3;2)", "2");
    check("=MODE(1;2;2;3;3;3)", "3");
    check("=MODE(1;2;3)", "#VALUE!");
    check("=VAR(A1:A4)", "1.6666666667");
    check("=VARP(A1:A4)", "1.25");
    check("=STDEVP(2;4;4;4;5;5;7;9)", "2");
    check("=STDEV(1)", "#DIV/0!");
    check("=LARGE(A1:A4;2)", "3");
    check("=SMALL(A1:A4;2)", "2");
    check("=LARGE(A1:A4;9)", "#VALUE!");
    check("=RANK(3;A1:A4)", "2");
    check("=RANK(3;A1:A4;1)", "3");
    check("=SUMSQ(A1:A3)", "14");
    check("=SUMPRODUCT(A1:A4;C1:C4)", "300");
    check("=SUMPRODUCT(A1:A4;C1:C3)", "#VALUE!");
    check("=GEOMEAN(2;8)", "4");
}

#[test]
fn conditional_aggregates() {
    check("=SUMIF(A1:A4;\">2\")", "7");
    check("=SUMIF(B1:B4;\"apple\";C1:C4)", "50");
    check("=SUMIF(B1:B4;\"a*\";C1:C4)", "50");
    check("=SUMIF(B1:B4;\"<>apple\";C1:C4)", "50");
    check("=COUNTIF(A1:A4;2)", "1");
    check("=COUNTIF(A1:A4;\">=2\")", "3");
    check("=COUNTIF(B1:B4;\"?anana\")", "1");
    check("=COUNTIF(B1:B4;\"BANANA\")", "1");
    check("=AVERAGEIF(A1:A4;\">2\")", "3.5");
    check("=AVERAGEIF(A1:A4;\">99\")", "#DIV/0!");
}

#[test]
fn math_functions() {
    check("=ABS(-3)", "3");
    check("=SIGN(-3)&SIGN(0)&SIGN(2)", "-101");
    check("=SQRT(16)", "4");
    check("=SQRT(-1)", "#NUM!");
    check("=LN(0)", "#NUM!");
    check("=EXP(0)", "1");
    check("=LOG(8;2)", "3");
    check("=LOG(100)", "2");
    check("=LOG10(1000)", "3");
    check("=POWER(2;8)", "256");
    check("=MOD(10;3)", "1");
    check("=MOD(-1;3)", "2");
    check("=MOD(1;0)", "#DIV/0!");
    check("=QUOTIENT(7;2)", "3");
    check("=INT(-1.5)", "-2");
    check("=TRUNC(-1.57;1)", "-1.5");
    check("=ROUND(2.5)", "3");
    check("=ROUND(-2.5)", "-3");
    check("=ROUND(1.005;2)", "1.01");
    check("=ROUND(1234.5;-2)", "1200");
    check("=ROUNDUP(1.21;1)", "1.3");
    check("=ROUNDUP(-1.21;1)", "-1.3");
    check("=ROUNDDOWN(1.29;1)", "1.2");
    check("=ROUNDUP(0.3;1)", "0.3");
    check("=CEILING(7;5)", "10");
    check("=FLOOR(7;5)", "5");
    check("=CEILING(1.2)", "2");
    check("=EVEN(3)&ODD(4)&EVEN(-1)&ODD(0)", "45-21");
    check("=FACT(5)", "120");
    check("=FACT(-1)", "#NUM!");
    check("=FACT(171)", "#NUM!");
    check("=GCD(12;18)", "6");
    check("=LCM(4;6)", "12");
    check("=COMBIN(5;2)", "10");
    check("=ROUND(PI();4)", "3.1416");
    check("=ROUND(SIN(PI()/2);6)", "1");
    check("=ROUND(DEGREES(PI());6)", "180");
    check("=ROUND(ATAN2(1;1)*4;6)", "3.141593");
    check("=AND(RAND()>=0;RAND()<1)", "TRUE");
    check("=AND(RANDBETWEEN(3;5)>=3;RANDBETWEEN(3;5)<=5)", "TRUE");
    check("=RANDBETWEEN(5;3)", "#VALUE!");
}

#[test]
fn logic_functions() {
    check("=IF(A1>0;\"pos\";\"neg\")", "pos");
    check("=IF(A1>5;\"big\")", "FALSE");
    check("=IF(TRUE)", "TRUE");
    check("=IF(FALSE;1/0;7)", "7");
    check("=IF(\"x\";1;2)", "#VALUE!");
    check("=AND(TRUE;1;A1:A4)", "TRUE");
    check("=AND(TRUE;0)", "FALSE");
    check("=OR(FALSE;0)", "FALSE");
    check("=XOR(TRUE;TRUE;TRUE)", "TRUE");
    check("=NOT(1)", "FALSE");
    check("=IFERROR(1/0;\"oops\")", "oops");
    check("=IFERROR(5;\"oops\")", "5");
    check("=IFNA(NA();1)", "1");
    check("=IFNA(1/0;1)", "#DIV/0!");
    check("=ISBLANK(Z99)", "TRUE");
    check("=ISBLANK(A1)", "FALSE");
    check("=ISNUMBER(A1)&ISNUMBER(B1)", "TRUEFALSE");
    check("=ISTEXT(B1)&ISNONTEXT(B1)", "TRUEFALSE");
    check("=ISLOGICAL(A6)", "TRUE");
    check("=ISERROR(D1)&ISERROR(D2)&ISERROR(A1)", "TRUETRUEFALSE");
    check("=ISERR(D1)&ISERR(D2)", "TRUEFALSE");
    check("=ISNA(D2)&ISNA(D1)", "TRUEFALSE");
    check("=ISEVEN(4)&ISODD(4)", "TRUEFALSE");
    check("=CHOOSE(2;\"a\";\"b\";\"c\")", "b");
    check("=CHOOSE(4;\"a\";\"b\";\"c\")", "#VALUE!");
    check("=NA()", "#N/A");
    check("=TRUE()", "TRUE");
}

#[test]
fn text_functions() {
    check("=CONCATENATE(\"a\";1;TRUE)", "a1TRUE");
    check("=LEN(\"héllo\")", "5");
    check("=LEFT(\"hello\";2)", "he");
    check("=LEFT(\"hello\")", "h");
    check("=RIGHT(\"hello\";3)", "llo");
    check("=RIGHT(\"hi\";10)", "hi");
    check("=MID(\"hello\";2;3)", "ell");
    check("=MID(\"hello\";0;3)", "#VALUE!");
    check("=UPPER(\"abc\")&LOWER(\"ABC\")", "ABCabc");
    check("=PROPER(\"hELLO wORLD-foo\")", "Hello World-Foo");
    check("=TRIM(\"  a   b  \")", "a b");
    check("=SUBSTITUTE(\"a-b-c\";\"-\";\"+\")", "a+b+c");
    check("=SUBSTITUTE(\"a-b-c\";\"-\";\"+\";2)", "a-b+c");
    check("=REPLACE(\"abcdef\";2;3;\"X\")", "aXef");
    check("=FIND(\"l\";\"hello\")", "3");
    check("=FIND(\"l\";\"hello\";4)", "4");
    check("=FIND(\"L\";\"hello\")", "#VALUE!");
    check("=SEARCH(\"L\";\"hello\")", "3");
    check("=REPT(\"ab\";3)", "ababab");
    check("=REPT(\"x\";1E9)", "#VALUE!");
    check("=EXACT(\"a\";\"A\")", "FALSE");
    check("=CHAR(65)&CODE(\"A\")", "A65");
    check("=VALUE(\"12.5\")+VALUE(\"50%\")", "13");
    check("=VALUE(\"x\")", "#VALUE!");
    check("=T(\"x\")&T(5)", "x");
    check("=N(\"x\")+N(5)", "5");
    check("=TEXT(1234.567;\"#,##0.00\")", "1,234.57");
    check("=TEXT(0.256;\"0.0%\")", "25.6%");
    check("=TEXT(E1;\"dd/mm/yyyy\")", "29/02/2024");
    check("=TEXT(E1;\"mmm d, yyyy\")", "Feb 29, 2024");
    check("=TEXT(E1+E2;\"hh:mm\")", "12:00");
    check("=TEXT(E1;\"dddd\")", "Thursday");
    check("=TEXTJOIN(\", \";TRUE;B1:B3;\"\";\"x\")", "apple, Banana, cherry, x");
}

#[test]
fn lookup_functions() {
    check("=VLOOKUP(3;A1:C4;3)", "30");
    check("=VLOOKUP(3.5;A1:C4;3)", "30");
    check("=VLOOKUP(3.5;A1:C4;3;FALSE)", "#N/A");
    check("=VLOOKUP(0;A1:C4;3)", "#N/A");
    check("=VLOOKUP(\"banana\";B1:C4;2;0)", "20");
    check("=VLOOKUP(1;A1:C4;4)", "#VALUE!");
    check("=HLOOKUP(1;A1:C4;2;0)", "2");
    check("=MATCH(3;A1:A4;0)", "3");
    check("=MATCH(\"cherry\";B1:B4;0)", "3");
    check("=MATCH(2.5;A1:A4)", "2");
    check("=MATCH(9;A1:A4;0)", "#N/A");
    check("=INDEX(A1:C4;2;3)", "20");
    check("=INDEX(A1:A4;3)", "3");
    check("=INDEX(A1:C1;3)", "10");
    check("=INDEX(A1:C4;5;1)", "#REF!");
    check("=INDEX(C1:C4;MATCH(\"cherry\";B1:B4;0))", "30");
    check("=ROW()&COLUMN()", "126");
    check("=ROW(B7)&COLUMN(B7)", "72");
    check("=ROWS(A1:C4)&COLUMNS(A1:C4)", "43");
}

#[test]
fn date_functions() {
    check("=YEAR(E1)&\"-\"&MONTH(E1)&\"-\"&DAY(E1)", "2024-2-29");
    check("=DATE(2024;2;29)=E1", "TRUE");
    check("=DATE(2024;14;1)=DATE(2025;2;1)", "TRUE");
    check("=DATE(2024;3;0)=E1", "TRUE");
    check("=WEEKDAY(E1)", "5");
    check("=WEEKDAY(E1;2)", "4");
    check("=WEEKDAY(E1;3)", "3");
    check("=HOUR(E2)&MINUTE(E2)&SECOND(E2)", "1200");
    check("=TIME(6;0;0)", "0.25");
    check("=DAYS(DATE(2024;3;1);DATE(2024;2;1))", "29");
    check("=EDATE(DATE(2024;1;31);1)=DATE(2024;2;29)", "TRUE");
    check("=EOMONTH(DATE(2024;1;15);1)=DATE(2024;2;29)", "TRUE");
    check("=TODAY()=E1", "TRUE");
    check("=NOW()-TODAY()", "0.75");
    check("=YEAR(-1E9)", "#NUM!");
}

#[test]
fn financial_functions() {
    check("=ROUND(PMT(0.05/12;360;200000);2)", "-1073.64");
    check("=PMT(0;10;1000)", "-100");
    check("=ROUND(FV(0.05;10;-100);2)", "1257.79");
    check("=ROUND(PV(0.05;10;-100);2)", "772.17");
    check("=ROUND(NPV(0.1;100;100;100);2)", "248.69");
}

#[test]
fn errors_for_bad_calls() {
    check("=NOSUCHFUNC(1)", "#NAME?");
    check("=somename", "#NAME?");
    check("=SUM(", "Err:509");
    check("=1 +", "Err:509");
    check("=ABS()", "Err:504");
    check("=ABS(1;2)", "Err:504");
    check("=PI(1)", "Err:504");
    check("=NoSheet.A1", "#REF!");
    check("=A1:B4", "#VALUE!");
}

#[test]
fn implicit_intersection_uses_the_formula_row() {
    let mut book = wb(&[("A1", "10"), ("A2", "20"), ("A3", "30")]);
    book.set_input(0, a("C2"), "=A1:A3*2").expect("set");
    assert_eq!(book.display(0, a("C2")), "40");
}

#[test]
fn dependents_recalculate_incrementally() {
    let mut book = wb(&[("A1", "1"), ("B1", "=A1+1"), ("C1", "=B1*2"), ("D1", "=SUM(A1:C1)"), ("E1", "=5")]);
    assert_eq!(book.display(0, a("D1")), "7");
    book.set_input(0, a("A1"), "10").expect("set");
    assert_eq!(book.display(0, a("B1")), "11");
    assert_eq!(book.display(0, a("C1")), "22");
    assert_eq!(book.display(0, a("D1")), "43");
    // Replacing a formula by a constant drops its old dependencies.
    book.set_input(0, a("B1"), "100").expect("set");
    book.set_input(0, a("A1"), "0").expect("set");
    assert_eq!(book.display(0, a("C1")), "200");
    assert_eq!(book.display(0, a("D1")), "300");
    // Clearing a precedent propagates too.
    book.clear_range(0, r("B1")).expect("clear");
    assert_eq!(book.display(0, a("C1")), "0");
}

#[test]
fn cross_sheet_references() {
    let mut book = wb(&[("A1", "5")]);
    let second = book.add_sheet(Some("Data 2")).expect("add");
    book.set_input(second, a("A1"), "=Sheet1.A1*2").expect("set");
    book.set_input(0, a("B1"), "='Data 2'.A1+1").expect("set");
    assert_eq!(book.display(0, a("B1")), "11");
    book.set_input(0, a("A1"), "7").expect("set");
    assert_eq!(book.display(0, a("B1")), "15");

    book.rename_sheet(second, "Renamed").expect("rename");
    assert_eq!(book.input_text(0, a("B1")), "=Renamed.A1+1");
    assert_eq!(book.display(0, a("B1")), "15");

    book.remove_sheet(second).expect("remove");
    assert_eq!(book.display(0, a("B1")), "#REF!");
    assert!(book.undo());
    assert_eq!(book.display(0, a("B1")), "15");

    assert!(book.add_sheet(Some("Sheet1")).is_err(), "duplicate");
    assert!(book.add_sheet(Some("sheet1")).is_err(), "duplicate ignoring case");
    assert!(book.add_sheet(Some("bad/name")).is_err());
    assert!(book.rename_sheet(0, "").is_err());
    assert!(book.remove_sheet(9).is_err());
}

#[test]
fn circular_references_are_detected() {
    let mut book = wb(&[("A1", "=B1"), ("B1", "=C1"), ("C1", "=A1"), ("D1", "=A1+1"), ("E1", "=E1")]);
    for cell in ["A1", "B1", "C1", "E1"] {
        assert_eq!(book.value(0, a(cell)), &Value::Error(ErrorKind::Circular), "{cell}");
    }
    assert!(book.value(0, a("D1")).is_error());
    // Breaking the cycle heals every cell.
    book.set_input(0, a("C1"), "4").expect("set");
    assert_eq!(book.display(0, a("A1")), "4");
    assert_eq!(book.display(0, a("D1")), "5");
    // A range that includes the formula itself is circular as well.
    book.set_input(0, a("F1"), "=SUM(F1:F3)").expect("set");
    assert_eq!(book.value(0, a("F1")), &Value::Error(ErrorKind::Circular));
}

#[test]
fn long_dependency_chains_do_not_overflow_the_stack() {
    let mut book = Workbook::new();
    let rows: Vec<Vec<String>> = (0..200_000u32)
        .map(|i| vec![if i == 0 { "1".to_owned() } else { format!("=A{i}+1") }])
        .collect();
    book.set_inputs(0, a("A1"), &rows).expect("bulk set");
    assert_eq!(book.display(0, a("A200000")), "200000");
    book.set_input(0, a("A1"), "5").expect("set");
    assert_eq!(book.display(0, a("A200000")), "200004");
}

#[test]
fn volatile_functions_recalculate_on_every_change() {
    let mut book = wb(&[("A1", "=RAND()"), ("B1", "=A1*2")]);
    book.seed_random(42);
    book.set_input(0, a("C1"), "1").expect("set");
    let first = book.value(0, a("A1")).clone();
    book.set_input(0, a("C1"), "2").expect("set");
    let second = book.value(0, a("A1")).clone();
    assert_ne!(first, second);
    match (book.value(0, a("A1")), book.value(0, a("B1"))) {
        (Value::Number(x), Value::Number(y)) => assert_eq!(*y, x * 2.0),
        other => panic!("{other:?}"),
    }
}

#[test]
fn text_growth_is_capped() {
    let mut book = wb(&[("A1", "xxxxxxxxxxxxxxxx")]);
    let rows: Vec<Vec<String>> = (1..40u32).map(|i| vec![format!("=A{i}&A{i}")]).collect();
    book.set_inputs(0, a("A2"), &rows).expect("bulk");
    assert_eq!(book.value(0, a("A40")), &Value::Error(ErrorKind::Value));
}

#[test]
fn input_interpretation_and_edit_text_round_trip() {
    let mut book = Workbook::new();
    let cases = [
        ("42", "42", "42"), ("0.1", "0.1", "0.1"), ("hello", "hello", "hello"), ("'007", "007", "'007"),
        ("true", "TRUE", "TRUE"), ("12.5%", "12.5%", "12.5%"), ("2024-02-29", "2024-02-29", "2024-02-29"),
        ("18:30", "18:30:00", "18:30:00"), ("$1,234.5", "$1,234.50", "1234.5"), ("=1+1", "2", "=1+1"),
        ("=sum(a1:a2)", "0", "=SUM(A1:A2)"), ("=1+", "Err:509", "=1+"), ("'=x", "=x", "'=x"),
        ("1,00,000", "1,00,000", "100000"), ("₹2,50,000.5", "₹2,50,000.50", "250000.5"),
        ("26-01-1950", "26-01-1950", "26-01-1950"),
    ];
    for (i, (input, display, edit)) in cases.iter().enumerate() {
        let addr = CellAddr::new(i as u32, 5);
        book.set_input(0, addr, input).expect("set");
        assert_eq!(book.display(0, addr), *display, "display of {input}");
        assert_eq!(book.input_text(0, addr), *edit, "edit text of {input}");
        // Re-entering the edit text must not change what is displayed.
        let again = book.input_text(0, addr);
        book.set_input(0, addr, &again).expect("set");
        assert_eq!(book.display(0, addr), *display, "re-entered {input}");
    }
    // A cell formatted as text keeps everything literally.
    book.set_style(0, r("H1"), &StylePatch { format: Some(NumberFormat::Text), ..Default::default() }).expect("style");
    book.set_input(0, a("H1"), "=1+1").expect("set");
    assert_eq!(book.value(0, a("H1")), &Value::Text("=1+1".into()));
    // Out-of-sheet addresses and unknown sheets are rejected.
    assert!(book.set_input(0, CellAddr::new(2_000_000, 0), "x").is_err());
    assert!(book.set_input(7, a("A1"), "x").is_err());
}

#[test]
fn undo_and_redo() {
    let mut book = wb(&[("A1", "1"), ("B1", "=A1*10")]);
    book.mark_saved();
    assert!(!book.is_modified());
    book.set_input(0, a("A1"), "2").expect("set");
    assert!(book.is_modified());
    book.set_style(0, r("A1:B1"), &StylePatch { bold: Some(true), ..Default::default() }).expect("style");
    assert_eq!(book.display(0, a("B1")), "20");
    assert!(book.style(0, a("B1")).bold);

    assert!(book.undo());
    assert!(!book.style(0, a("B1")).bold);
    assert!(book.undo());
    assert_eq!(book.display(0, a("B1")), "10");
    assert!(book.redo());
    assert_eq!(book.display(0, a("B1")), "20");
    assert!(book.redo());
    assert!(book.style(0, a("A1")).bold);
    assert!(!book.redo());

    // A new edit clears the redo stack.
    assert!(book.undo());
    book.set_input(0, a("C1"), "x").expect("set");
    assert!(!book.can_redo());

    // Setting the same value again is not an undo step.
    let mut book = wb(&[("A1", "1")]);
    while book.undo() {}
    book.set_input(0, a("A1"), "1").expect("set");
    book.set_input(0, a("A1"), "1").expect("set");
    assert!(book.undo());
    assert!(!book.can_undo());
}

#[test]
fn undo_history_is_bounded() {
    let mut book = Workbook::new();
    for i in 0..(office_calc::MAX_UNDO + 50) {
        book.set_input(0, a("A1"), &i.to_string()).expect("set");
    }
    let mut steps = 0;
    while book.undo() {
        steps += 1;
    }
    assert_eq!(steps, office_calc::MAX_UNDO);
}

#[test]
fn inserting_and_deleting_rows_adjusts_references() {
    let mut book = wb(&[
        ("A1", "1"), ("A2", "2"), ("A3", "3"), ("A4", "=SUM(A1:A3)"), ("B1", "=A3"), ("B2", "=$A$2+A1"),
    ]);
    book.insert(0, Axis::Row, 1, 2).expect("insert rows");
    assert_eq!(book.input_text(0, a("A6")), "=SUM(A1:A5)");
    assert_eq!(book.display(0, a("A6")), "6");
    assert_eq!(book.input_text(0, a("B1")), "=A5");
    assert_eq!(book.input_text(0, a("B4")), "=$A$4+A1");
    assert_eq!(book.display(0, a("A4")), "2");

    book.delete(0, Axis::Row, 1, 2).expect("delete rows");
    assert_eq!(book.input_text(0, a("A4")), "=SUM(A1:A3)");
    assert_eq!(book.input_text(0, a("B1")), "=A3");

    // Deleting a referenced row yields #REF!, and ranges shrink.
    book.delete(0, Axis::Row, 2, 1).expect("delete row 3");
    assert_eq!(book.input_text(0, a("B1")), "=#REF!");
    assert_eq!(book.display(0, a("B1")), "#REF!");
    assert_eq!(book.input_text(0, a("A3")), "=SUM(A1:A2)");
    assert_eq!(book.display(0, a("A3")), "3");

    // Structural edits are undoable as one step.
    assert!(book.undo());
    assert_eq!(book.input_text(0, a("B1")), "=A3");
    assert_eq!(book.display(0, a("A4")), "6");

    // Deleting the whole range turns it into #REF!.
    book.delete(0, Axis::Row, 0, 3).expect("delete all");
    assert_eq!(book.input_text(0, a("A1")), "=SUM(#REF!)");
}

#[test]
fn inserting_and_deleting_columns_adjusts_references_and_widths() {
    let mut book = wb(&[("A1", "1"), ("B1", "2"), ("C1", "=A1+B1"), ("D1", "=SUM(A1:B1)")]);
    book.set_col_width(0, 1, Some(120.0)).expect("width");
    book.insert(0, Axis::Col, 1, 1).expect("insert col");
    assert_eq!(book.input_text(0, a("D1")), "=A1+C1");
    assert_eq!(book.input_text(0, a("E1")), "=SUM(A1:C1)");
    assert_eq!(book.display(0, a("E1")), "3");
    assert_eq!(book.sheet(0).expect("sheet").col_widths.get(&2), Some(&120.0));
    book.delete(0, Axis::Col, 0, 1).expect("delete col");
    assert_eq!(book.input_text(0, a("C1")), "=#REF!+B1");
    assert_eq!(book.input_text(0, a("D1")), "=SUM(A1:B1)");

    assert!(book.insert(0, Axis::Col, 5000, 1).is_err());
    assert!(book.insert(0, Axis::Row, 0, 0).is_err());
    // Inserting must not push data off the sheet.
    book.set_input(0, CellAddr::new(office_calc::MAX_ROWS - 1, 0), "edge").expect("set");
    assert!(book.insert(0, Axis::Row, 0, 1).is_err());
}

#[test]
fn references_from_other_sheets_follow_structural_edits() {
    let mut book = wb(&[("A1", "1"), ("A2", "2")]);
    let other = book.add_sheet(None).expect("add");
    book.set_input(other, a("A1"), "=Sheet1.A2*10").expect("set");
    book.set_input(other, a("A2"), "=A1").expect("set");
    book.insert(0, Axis::Row, 0, 3).expect("insert");
    assert_eq!(book.input_text(other, a("A1")), "=Sheet1.A5*10");
    assert_eq!(book.input_text(other, a("A2")), "=A1", "local reference on another sheet is untouched");
    assert_eq!(book.display(other, a("A1")), "20");
}

#[test]
fn copy_fill_and_sort() {
    let mut book = wb(&[("A1", "1"), ("A2", "2"), ("A3", "3"), ("B1", "=A1*$A$1+1")]);
    book.fill(0, r("B1"), r("B1:B3")).expect("fill down");
    assert_eq!(book.input_text(0, a("B3")), "=A3*$A$1+1");
    assert_eq!(book.display(0, a("B3")), "4");

    book.copy_range(0, r("A1:B3"), 0, a("D5")).expect("copy");
    assert_eq!(book.input_text(0, a("E7")), "=D7*$A$1+1");
    assert_eq!(book.display(0, a("E7")), "4");
    // Copying towards the edge turns impossible references into #REF!.
    book.set_input(0, a("C5"), "=A1").expect("set");
    book.copy_range(0, r("C5"), 0, a("A2")).expect("copy");
    assert_eq!(book.input_text(0, a("A2")), "=#REF!");
    assert!(book.undo());

    let mut book = wb(&[
        ("A1", "pear"), ("B1", "3"), ("A2", "apple"), ("B2", "1"), ("A3", ""), ("B3", ""), ("A4", "fig"),
        ("B4", "2"), ("C1", "=B1*2"), ("C2", "=B2*2"), ("C4", "=B4*2"),
    ]);
    book.sort_range(0, r("A1:C4"), 1, true).expect("sort");
    let column = |book: &Workbook, col: &str| -> Vec<String> {
        (1..=4).map(|i| book.display(0, a(&format!("{col}{i}")))).collect()
    };
    assert_eq!(column(&book, "A"), ["apple", "fig", "pear", ""]);
    assert_eq!(column(&book, "C"), ["2", "4", "6", ""]);
    book.sort_range(0, r("A1:C4"), 0, false).expect("sort desc");
    assert_eq!(column(&book, "A"), ["pear", "fig", "apple", ""]);
    assert_eq!(column(&book, "C"), ["6", "4", "2", ""]);
    assert!(book.undo());
    assert_eq!(column(&book, "A"), ["apple", "fig", "pear", ""]);
    assert!(book.sort_range(0, r("A1:C4"), 9, true).is_err());
    assert!(book.fill(0, r("B1"), r("C1:C3")).is_err());
}

#[test]
fn styles_and_number_formats() {
    let mut book = wb(&[("A1", "1234.5"), ("A2", "0.256")]);
    let patch = StylePatch {
        bold: Some(true),
        align: Some(HAlign::Center),
        format: Some(NumberFormat::Number { decimals: 2, thousands: true }),
        ..Default::default()
    };
    book.set_style(0, r("A1"), &patch).expect("style");
    assert_eq!(book.display(0, a("A1")), "1,234.50");
    assert!(book.style(0, a("A1")).bold);
    assert_eq!(book.style(0, a("A1")).align, HAlign::Center);
    assert!(!book.style(0, a("A2")).bold);
    // Clearing contents keeps the style; a styled empty cell is kept.
    book.clear_range(0, r("A1")).expect("clear");
    assert!(book.style(0, a("A1")).bold);
    assert_eq!(book.display(0, a("A1")), "");
    // Oversized ranges are refused instead of allocating millions of cells.
    assert!(book.set_style(0, r("A1:AMJ1048576"), &patch).is_err());
}

#[test]
fn bulk_paste_is_one_undo_step_and_clips_at_the_edge() {
    let mut book = Workbook::new();
    let rows = vec![vec!["1".to_owned(), "2".to_owned()], vec!["=A1+B1".to_owned(), "x".to_owned()]];
    book.set_inputs(0, a("A1"), &rows).expect("paste");
    assert_eq!(book.display(0, a("A2")), "3");
    assert!(book.undo());
    assert_eq!(book.sheet(0).expect("sheet").cell_count(), 0);
    book.set_inputs(0, CellAddr::new(0, office_calc::MAX_COLS - 1), &rows).expect("paste at edge");
    assert_eq!(book.sheet(0).expect("sheet").cell_count(), 2);
}

#[test]
fn map_text_changes_only_text_cells_in_one_undo_step() {
    let mut book = wb(&[("A1", "rAma"), ("A2", "42"), ("A3", "=A2*2"), ("B1", "sItA"), ("C1", "outside")]);
    let n = book.map_text(0, r("A1:B3"), &|s| s.to_uppercase()).expect("map");
    assert_eq!(n, 2);
    assert_eq!(book.display(0, a("A1")), "RAMA");
    assert_eq!(book.display(0, a("B1")), "SITA");
    assert_eq!(book.input_text(0, a("A3")), "=A2*2");
    assert_eq!(book.display(0, a("C1")), "outside");
    assert!(book.undo());
    assert_eq!(book.display(0, a("A1")), "rAma");
    assert_eq!(book.display(0, a("B1")), "sItA");
}

#[test]
fn set_content_and_used_range() {
    let mut book = Workbook::new();
    assert_eq!(book.sheet(0).expect("sheet").used_range(), None);
    book.set_content(0, a("C3"), Content::Number(1.5)).expect("set");
    book.set_content(0, a("B5"), Content::Text("x".into())).expect("set");
    assert_eq!(book.sheet(0).expect("sheet").used_range(), Some(r("B3:C5")));
    assert_eq!(book.value(0, a("C3")), &Value::Number(1.5));
}
