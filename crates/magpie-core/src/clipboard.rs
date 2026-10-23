//! Wayland clipboard through wl-clipboard (`wl-copy`, `wl-paste`).

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};

const PNG: &str = "image/png";

pub fn copy_png(path: &Path) -> Result<()> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    // wl-copy reads stdin to the end, then forks to serve the selection, so
    // waiting here returns as soon as the copy is in place.
    let status = Command::new("wl-copy")
        .args(["--type", PNG])
        .stdin(file)
        .status()
        .context("running wl-copy")?;
    if !status.success() {
        bail!("wl-copy failed: {status}");
    }
    Ok(())
}

/// Puts PNG bytes on the clipboard. wl-copy keeps serving them after this
/// process exits, which a window-owned clipboard on Wayland would not.
pub fn copy_png_bytes(png: &[u8]) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .args(["--type", PNG])
        .stdin(Stdio::piped())
        .spawn()
        .context("running wl-copy")?;
    child.stdin.take().expect("stdin is piped").write_all(png)?;
    let status = child.wait()?;
    if !status.success() {
        bail!("wl-copy failed: {status}");
    }
    Ok(())
}

/// The clipboard's PNG. niri sets the selection just before it reports the
/// screenshot, so allow a short wait for it to appear.
pub fn paste_png() -> Result<Vec<u8>> {
    for _ in 0..20 {
        let types = Command::new("wl-paste")
            .arg("--list-types")
            .stderr(Stdio::null())
            .output()
            .context("running wl-paste")?;
        if String::from_utf8_lossy(&types.stdout)
            .lines()
            .any(|t| t == PNG)
        {
            let out = Command::new("wl-paste")
                .args(["--no-newline", "--type", PNG])
                .output()
                .context("running wl-paste")?;
            if out.status.success() && !out.stdout.is_empty() {
                return Ok(out.stdout);
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    bail!("clipboard has no {PNG}")
}
