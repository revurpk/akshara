//! Renders a Mermaid file to SVG or PNG (for checking diagrams by eye):
//!
//! ```text
//! cargo run -p office-draw --example mermaid -- diagram.mmd out.png
//! ```

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: mermaid <input.mmd> <output.svg|output.png>");
        return ExitCode::from(2);
    };
    let source = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{input}: {e}");
            return ExitCode::FAILURE;
        }
    };
    // The app's bundled fonts, when run from the repository.
    for dir in ["app/assets/fonts", "../../app/assets/fonts"] {
        if std::path::Path::new(dir).is_dir() {
            office_draw::render::add_font_dir(dir);
        }
    }
    let rendered = office_draw::mermaid::render(&source);
    for e in &rendered.errors {
        eprintln!("line {}: {}", e.line, e.message);
    }
    let bytes = if output.ends_with(".svg") {
        rendered.figure.to_svg(Some(office_odf::Rgb(255, 255, 255))).into_bytes()
    } else {
        match office_draw::render::figure_to_png(&rendered.figure, 2.0, Some(office_odf::Rgb(255, 255, 255))) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        }
    };
    if let Err(e) = std::fs::write(output, bytes) {
        eprintln!("{output}: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
