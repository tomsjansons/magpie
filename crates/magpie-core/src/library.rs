use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local};
use image::ImageFormat;
use serde::Serialize;

use crate::model::{ORIGINAL, Shot, Variant};
use crate::render;
use crate::shapes::Annotation;
use crate::target::Target;

/// Longest edge of the generated thumbnails, in pixels. The panel shows them
/// at roughly a third of this, so they stay sharp on scaled outputs.
const THUMB_SIZE: u32 = 480;

/// The on-disk screenshot library:
///
/// ```text
/// <root>/shots/<id>/meta.json        Shot
/// <root>/shots/<id>/original.png     only for clipboard captures
/// <root>/shots/<id>/<version>.png    rendered variant
/// <root>/shots/<id>/<version>.json   variant shape list
/// <root>/shots/<id>/<version>.thumb.png
/// ```
pub struct Library {
    root: PathBuf,
}

/// Result of an import: the shot, and whether it was new or already known.
pub struct Added {
    pub shot: Shot,
    pub new: bool,
}

/// A shot with every path resolved, as printed by `magpie list --json`.
#[derive(Debug, Serialize)]
pub struct ShotView {
    pub id: String,
    pub taken_at: DateTime<Local>,
    pub original: PathBuf,
    pub original_exists: bool,
    pub thumb: Option<PathBuf>,
    pub variants: Vec<VariantView>,
}

#[derive(Debug, Serialize)]
pub struct VariantView {
    pub id: String,
    pub parent: String,
    pub created: DateTime<Local>,
    pub png: PathBuf,
    pub thumb: Option<PathBuf>,
}

impl Library {
    /// `$MAGPIE_DATA_DIR`, else `$XDG_DATA_HOME/magpie`, else
    /// `~/.local/share/magpie`.
    pub fn open_default() -> Result<Self> {
        let root = if let Some(dir) = std::env::var_os("MAGPIE_DATA_DIR") {
            PathBuf::from(dir)
        } else if let Some(dir) = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
            PathBuf::from(dir).join("magpie")
        } else {
            let home = std::env::var_os("HOME").context("HOME is not set")?;
            PathBuf::from(home).join(".local/share/magpie")
        };
        Self::open(root)
    }

    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let shots = root.join("shots");
        fs::create_dir_all(&shots).with_context(|| format!("creating {}", shots.display()))?;
        Ok(Library { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Import a screenshot file in place. Importing the same file again
    /// returns the existing shot.
    pub fn add_file(&self, path: &Path) -> Result<Added> {
        let path = fs::canonicalize(path).with_context(|| format!("reading {}", path.display()))?;
        if let Some(shot) = self
            .shots()?
            .into_iter()
            .find(|s| s.source.as_deref() == Some(&path))
        {
            return Ok(Added { shot, new: false });
        }

        let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let taken_at = fs::metadata(&path)
            .and_then(|m| m.modified())
            .map(DateTime::<Local>::from)
            .unwrap_or_else(|_| Local::now());

        let shot = Shot {
            id: self.fresh_id(taken_at),
            source: Some(path),
            taken_at,
            variants: Vec::new(),
        };
        self.create(&shot, &bytes, None)?;
        Ok(Added { shot, new: true })
    }

    /// Import PNG bytes that exist nowhere else, such as a clipboard-only
    /// screenshot. The original is stored in the shot directory.
    pub fn add_png(&self, bytes: &[u8]) -> Result<Shot> {
        if image::guess_format(bytes).ok() != Some(ImageFormat::Png) {
            bail!("data is not a PNG image");
        }
        let taken_at = Local::now();
        let shot = Shot {
            id: self.fresh_id(taken_at),
            source: None,
            taken_at,
            variants: Vec::new(),
        };
        self.create(&shot, bytes, Some(bytes))?;
        Ok(shot)
    }

    /// Every shot, newest first. Unreadable entries are skipped so one broken
    /// directory cannot hide the rest of the library.
    pub fn shots(&self) -> Result<Vec<Shot>> {
        let mut shots = Vec::new();
        for entry in fs::read_dir(self.root.join("shots"))? {
            let entry = entry?;
            if let Ok(shot) = read_meta(&entry.path().join("meta.json")) {
                shots.push(shot);
            }
        }
        shots.sort_by(|a, b| b.taken_at.cmp(&a.taken_at).then_with(|| b.id.cmp(&a.id)));
        Ok(shots)
    }

    pub fn shot(&self, id: &str) -> Result<Shot> {
        if id.contains(['/', '\\']) || id.starts_with('.') {
            bail!("invalid shot id {id:?}");
        }
        read_meta(&self.shot_dir(id).join("meta.json")).with_context(|| format!("no shot {id:?}"))
    }

    /// The PNG for a shot version.
    pub fn resolve(&self, target: &Target) -> Result<PathBuf> {
        let shot = self.shot(&target.shot)?;
        let path = if target.is_original() {
            self.original_path(&shot)
        } else if shot.variant(&target.version).is_some() {
            self.shot_dir(&shot.id)
                .join(format!("{}.png", target.version))
        } else {
            bail!("shot {} has no version {:?}", shot.id, target.version);
        };
        if !path.exists() {
            bail!(
                "{target} points at {}, which no longer exists",
                path.display()
            );
        }
        Ok(path)
    }

    pub fn view(&self, shot: &Shot) -> ShotView {
        let dir = self.shot_dir(&shot.id);
        let original = self.original_path(shot);
        ShotView {
            id: shot.id.clone(),
            taken_at: shot.taken_at,
            original_exists: original.exists(),
            original,
            thumb: existing(dir.join(format!("{ORIGINAL}.thumb.png"))),
            variants: shot
                .variants
                .iter()
                .map(|v| VariantView {
                    id: v.id.clone(),
                    parent: v.parent.clone(),
                    created: v.created,
                    png: dir.join(format!("{}.png", v.id)),
                    thumb: existing(dir.join(format!("{}.thumb.png", v.id))),
                })
                .collect(),
        }
    }

    /// The shapes of a version: none for the original.
    pub fn annotation(&self, target: &Target) -> Result<Annotation> {
        let shot = self.shot(&target.shot)?;
        if target.is_original() {
            return Ok(Annotation::default());
        }
        if shot.variant(&target.version).is_none() {
            bail!("shot {} has no version {:?}", shot.id, target.version);
        }
        let path = self
            .shot_dir(&shot.id)
            .join(format!("{}.json", target.version));
        let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
    }

    /// Render `annotation` onto the shot's original and store it as the next
    /// version (`v1`, `v2`, ...) with the given parent.
    pub fn save_variant(
        &self,
        shot_id: &str,
        parent: &str,
        annotation: &Annotation,
    ) -> Result<Variant> {
        let shot = self.shot(shot_id)?;
        if parent != ORIGINAL && shot.variant(parent).is_none() {
            bail!("shot {shot_id} has no version {parent:?}");
        }
        let base = image::open(self.original_path(&shot)).context("opening the original")?;
        let rendered = render::render(&base.to_rgba8(), annotation)?;

        // Re-read right before writing, so a save from another editor window
        // in the meantime is not lost.
        let mut shot = self.shot(shot_id)?;
        let next = shot
            .variants
            .iter()
            .filter_map(|v| v.id.strip_prefix('v')?.parse::<u32>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let variant = Variant {
            id: format!("v{next}"),
            parent: parent.to_string(),
            created: Local::now(),
        };
        let dir = self.shot_dir(shot_id);
        write_atomic(
            &dir.join(format!("{}.json", variant.id)),
            &serde_json::to_vec_pretty(annotation)?,
        )?;
        write_atomic(
            &dir.join(format!("{}.png", variant.id)),
            &render::encode_png(&rendered)?,
        )?;
        let thumb = thumbnail(&image::DynamicImage::ImageRgba8(rendered))?;
        write_atomic(&dir.join(format!("{}.thumb.png", variant.id)), &thumb)?;
        shot.variants.push(variant.clone());
        write_atomic(&dir.join("meta.json"), &serde_json::to_vec_pretty(&shot)?)?;
        Ok(variant)
    }

    pub fn original_path(&self, shot: &Shot) -> PathBuf {
        match &shot.source {
            Some(path) => path.clone(),
            None => self.shot_dir(&shot.id).join(format!("{ORIGINAL}.png")),
        }
    }

    fn shot_dir(&self, id: &str) -> PathBuf {
        self.root.join("shots").join(id)
    }

    /// `YYYYmmdd-HHMMSS`, with `-2`, `-3`, ... when several shots land in the
    /// same second.
    fn fresh_id(&self, taken_at: DateTime<Local>) -> String {
        let base = taken_at.format("%Y%m%d-%H%M%S").to_string();
        let mut id = base.clone();
        let mut n = 2;
        while self.shot_dir(&id).exists() {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }

    /// Write a new shot directory. The thumbnail is made before anything is
    /// written, so a file that is not an image never becomes a shot.
    fn create(&self, shot: &Shot, image_bytes: &[u8], store_original: Option<&[u8]>) -> Result<()> {
        let thumb = thumbnail(&image::load_from_memory(image_bytes).context("decoding image")?)?;
        let dir = self.shot_dir(&shot.id);
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        if let Some(bytes) = store_original {
            write_atomic(&dir.join(format!("{ORIGINAL}.png")), bytes)?;
        }
        write_atomic(&dir.join(format!("{ORIGINAL}.thumb.png")), &thumb)?;
        write_atomic(&dir.join("meta.json"), &serde_json::to_vec_pretty(shot)?)
    }
}

fn read_meta(path: &Path) -> Result<Shot> {
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn existing(path: PathBuf) -> Option<PathBuf> {
    path.exists().then_some(path)
}

fn thumbnail(img: &image::DynamicImage) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    img.thumbnail(THUMB_SIZE, THUMB_SIZE)
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)?;
    Ok(out)
}

/// Write via a temp file and rename, so readers never see a partial file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbaImage::new(w, h)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn add_file_imports_in_place_and_dedups() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path().join("lib")).unwrap();
        let shot_path = tmp.path().join("Screenshot.png");
        fs::write(&shot_path, png(1920, 1080)).unwrap();

        let first = lib.add_file(&shot_path).unwrap();
        assert!(first.new);
        let again = lib.add_file(&shot_path).unwrap();
        assert!(!again.new);
        assert_eq!(first.shot.id, again.shot.id);

        let target: Target = first.shot.id.parse().unwrap();
        assert_eq!(
            lib.resolve(&target).unwrap(),
            fs::canonicalize(&shot_path).unwrap()
        );

        let view = lib.view(&first.shot);
        let thumb = image::open(view.thumb.unwrap()).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (480, 270));
    }

    #[test]
    fn add_png_stores_original_and_sorts_newest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        let a = lib.add_png(&png(10, 10)).unwrap();
        let b = lib.add_png(&png(10, 10)).unwrap();
        assert_ne!(a.id, b.id);

        let ids: Vec<_> = lib.shots().unwrap().into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![b.id.clone(), a.id.clone()]);
        assert!(
            lib.resolve(&b.id.parse().unwrap())
                .unwrap()
                .ends_with("original.png")
        );
    }

    #[test]
    fn rejects_non_images_without_leaving_a_shot() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        assert!(lib.add_png(b"not a png").is_err());
        let txt = tmp.path().join("notes.png");
        fs::write(&txt, "hello").unwrap();
        assert!(lib.add_file(&txt).is_err());
        assert!(lib.shots().unwrap().is_empty());
    }

    #[test]
    fn variants_number_up_and_keep_their_shapes() {
        use crate::shapes::{Color, Point, Shape};

        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        let shot = lib.add_png(&png(64, 48)).unwrap();
        let box_shape = Shape::Rect {
            from: Point::new(4.0, 4.0),
            to: Point::new(30.0, 30.0),
            color: Color::rgb(255, 0, 0),
            width: 3.0,
        };
        let first = Annotation {
            shapes: vec![box_shape.clone()],
            frame: None,
        };
        let v1 = lib.save_variant(&shot.id, ORIGINAL, &first).unwrap();
        assert_eq!((v1.id.as_str(), v1.parent.as_str()), ("v1", ORIGINAL));

        let v1_target: Target = format!("{}@v1", shot.id).parse().unwrap();
        let mut second = lib.annotation(&v1_target).unwrap();
        assert_eq!(second, first);
        second.shapes.push(box_shape);
        let v2 = lib.save_variant(&shot.id, "v1", &second).unwrap();
        assert_eq!((v2.id.as_str(), v2.parent.as_str()), ("v2", "v1"));

        let rendered = image::open(lib.resolve(&v1_target).unwrap()).unwrap();
        assert_eq!((rendered.width(), rendered.height()), (64, 48));
        assert_eq!(rendered.to_rgba8().get_pixel(4, 15).0[0], 255);
        let view = lib.view(&lib.shot(&shot.id).unwrap());
        assert_eq!(view.variants.len(), 2);
        assert!(view.variants.iter().all(|v| v.thumb.is_some()));
        assert!(
            lib.annotation(&shot.id.parse().unwrap())
                .unwrap()
                .shapes
                .is_empty()
        );
        assert!(lib.save_variant(&shot.id, "v7", &second).is_err());
    }

    #[test]
    fn unknown_versions_and_bad_ids_error() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        let shot = lib.add_png(&png(4, 4)).unwrap();
        assert!(
            lib.resolve(&format!("{}@v9", shot.id).parse().unwrap())
                .is_err()
        );
        assert!(lib.shot("../etc").is_err());
    }
}
