//! Screenshot library storage, the annotation shape model, and the tiny-skia
//! renderer used for export. Shared by the CLI and the editor.

mod library;
mod model;
mod target;

pub use library::{Added, Library, ShotView, VariantView};
pub use model::{ORIGINAL, Shot, Variant};
pub use target::Target;
