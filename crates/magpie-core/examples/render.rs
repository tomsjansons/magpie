//! Render a shape list onto an image, the same way saving a version does.
//!
//!     cargo run -p magpie-core --example render -- base.png shapes.json out.png

use anyhow::{Context, Result};
use magpie_core::render;
use magpie_core::shapes::Annotation;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [base, shapes, out] = args.as_slice() else {
        anyhow::bail!("usage: render <base.png> <shapes.json> <out.png>");
    };
    let base = image::open(base).context("opening base image")?.to_rgba8();
    let annotation: Annotation = serde_json::from_slice(&std::fs::read(shapes)?)?;
    std::fs::write(
        out,
        render::encode_png(&render::render(&base, &annotation)?)?,
    )?;
    Ok(())
}
