//! Render bbcode into egui widgets.
//!
//! ```ignore
//! let mut state = bbcode_egui::BBState::default();
//! bbcode_egui::BBView::new(text).show(ui, &mut state);
//! ```
//!
//! Tags outside the supported set render as their raw bracketed form, so
//! unrecognized markup stays visible as text. Link URLs are restricted to
//! `http`, `https`, and `mailto`; image URLs to `http` and `https`.
//!
//! With the `entities` feature, HTML numeric (`&#N;`, `&#xN;`) and a small
//! set of named entities (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`,
//! `&nbsp;`) are decoded in text content, and control / bidi formatting
//! characters are stripped.

mod decode;
mod ir;
mod render;
mod sanitize;
mod style;

pub use render::{BBState, BBView};
