//! Screenshot library storage, the annotation shape model, and the tiny-skia
//! renderer used for export. Shared by the CLI and the editor.

pub mod clipboard;
mod library;
mod model;
pub mod render;
pub mod shapes;
mod target;

pub use library::{Added, Library, ShotView, VariantView};
pub use model::{ORIGINAL, Shot, Variant};
pub use target::Target;
