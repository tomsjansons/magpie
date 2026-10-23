//! Toolbar icons: Lucide, through GPUI Kit's assets. The ones outside Kit's
//! default set are bundled here.

use std::borrow::Cow;

use gpui_kit::assets::{Assets, IconNamed};
use gpui_kit::{AssetSource, Result, SharedString};

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [MoveUpRight, Circle, Type, MousePointer2, Save, Trash]
);

/// Kit's default assets plus [`ExtraIcons`].
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Icon {
    MousePointer,
    Arrow,
    Rect,
    Ellipse,
    Text,
    Undo,
    Redo,
    Trash,
    Copy,
    Save,
}

impl IconNamed for Icon {
    fn path(self) -> SharedString {
        match self {
            Icon::MousePointer => "icons/mouse-pointer-2.svg",
            Icon::Arrow => "icons/move-up-right.svg",
            Icon::Rect => "icons/square.svg",
            Icon::Ellipse => "icons/circle.svg",
            Icon::Text => "icons/type.svg",
            Icon::Undo => "icons/undo-2.svg",
            Icon::Redo => "icons/redo-2.svg",
            Icon::Trash => "icons/trash.svg",
            Icon::Copy => "icons/copy.svg",
            Icon::Save => "icons/save.svg",
        }
        .into()
    }
}
