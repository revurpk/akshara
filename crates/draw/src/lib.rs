//! # office-draw
//!
//! The core of Akshara Impress (presentations) and Akshara Draw (vector
//! drawings), also used for pictures, drawings and diagrams in Writer.
//!
//! * [`model`]: shapes (rectangles, ellipses, lines with arrows, polygons,
//!   text boxes, pictures, Mermaid diagrams) and pages;
//! * [`Drawing`]: a presentation or drawing with validated editing and
//!   undo/redo;
//! * [`Image`]: embedded pictures (PNG, JPEG, GIF, WebP, BMP, SVG), kept
//!   byte for byte;
//! * [`mermaid`]: native Mermaid rendering (flowcharts, sequence, class and
//!   state diagrams, pie charts) into vector [`Figure`]s;
//! * [`odg`]: OpenDocument Presentation (`.odp`) and Graphics (`.odg`)
//!   import and export;
//! * [`render`]: SVG and PNG output.
//!
//! The crate contains no `unsafe` code.
//!
//! ```
//! use office_draw::{Drawing, DrawingKind, Shape, ShapeKind};
//!
//! let mut doc = Drawing::new(DrawingKind::Drawing);
//! let ids = doc.add_shapes(0, vec![Shape::new(ShapeKind::Ellipse, 72.0, 72.0, 144.0, 72.0)]).unwrap();
//! let bytes = office_draw::odg::save(&doc).unwrap();
//! let back = office_draw::odg::load(&bytes, &Default::default()).unwrap();
//! assert_eq!(back.pages()[0].shapes.len(), 1);
//! assert_eq!(ids.len(), 1);
//! ```

mod drawing;
pub mod figure;
pub mod image;
pub mod mermaid;
pub mod model;
pub mod odg;
pub mod render;

pub use drawing::{layout_shapes, Drawing, DrawingKind, Layout, DRAWING_SIZE, MAX_PAGES, SLIDE_SIZE};
pub use figure::Figure;
pub use image::{Image, ImageFormat};
pub use model::{HAlign, Page, Placeholder, Shape, ShapeKind, ShapePatch, TextStyle, VAlign, ZOrder};
