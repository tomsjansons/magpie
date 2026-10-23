use std::path::PathBuf;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

/// Version name for the untouched screenshot, used as a variant's `parent`.
pub const ORIGINAL: &str = "original";

/// One screenshot and its annotated variants, stored as `shots/<id>/meta.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shot {
    pub id: String,
    /// Where niri wrote the original. `None` when it came from the clipboard,
    /// in which case the original lives in the shot directory.
    pub source: Option<PathBuf>,
    pub taken_at: DateTime<Local>,
    #[serde(default)]
    pub variants: Vec<Variant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variant {
    /// `v1`, `v2`, ... in creation order.
    pub id: String,
    /// `original` or the id of the variant this one was started from.
    pub parent: String,
    pub created: DateTime<Local>,
}

impl Shot {
    pub fn variant(&self, id: &str) -> Option<&Variant> {
        self.variants.iter().find(|v| v.id == id)
    }
}
