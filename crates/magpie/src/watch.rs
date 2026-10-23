//! Follow `niri msg --json event-stream` and import every screenshot.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use magpie_core::Library;
use serde::Serialize;
use serde_json::Value;

use crate::clipboard;

/// One line of `magpie watch` output.
#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Out {
    Ready,
    Added { id: String, new: bool },
    Error { message: String },
}

fn emit(out: &Out) {
    // Rust's stdout is line-buffered, so each event reaches the reader at once.
    println!("{}", serde_json::to_string(out).expect("event serializes"));
}

pub fn run(lib: &Library, niri: &str) -> Result<()> {
    let mut child = Command::new(niri)
        .args(["msg", "--json", "event-stream"])
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("running {niri} msg event-stream"))?;
    let stdout = child.stdout.take().expect("stdout is piped");
    emit(&Out::Ready);

    for line in BufReader::new(stdout).lines() {
        let line = line.context("reading niri event stream")?;
        let Some(path) = screenshot_event(&line) else {
            continue;
        };
        let result = match path {
            Some(path) => lib.add_file(Path::new(&path)).map(|a| (a.shot.id, a.new)),
            None => clipboard::paste_png()
                .and_then(|png| lib.add_png(&png))
                .map(|shot| (shot.id, true)),
        };
        emit(&match result {
            Ok((id, new)) => Out::Added { id, new },
            Err(err) => Out::Error {
                message: format!("{err:#}"),
            },
        });
    }

    let status = child.wait()?;
    bail!("niri event stream ended: {status}")
}

/// `Some(path)` for a `ScreenshotCaptured` event, where `path` is `None` for a
/// clipboard-only capture. `None` for every other event.
fn screenshot_event(line: &str) -> Option<Option<String>> {
    let event: Value = serde_json::from_str(line).ok()?;
    let captured = event.get("ScreenshotCaptured")?;
    Some(
        captured
            .get("path")
            .and_then(Value::as_str)
            .map(str::to_string),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_out_screenshot_events() {
        assert_eq!(
            screenshot_event(r#"{"ScreenshotCaptured":{"path":"/tmp/a.png"}}"#),
            Some(Some("/tmp/a.png".to_string()))
        );
        assert_eq!(
            screenshot_event(r#"{"ScreenshotCaptured":{"path":null}}"#),
            Some(None)
        );
        assert_eq!(
            screenshot_event(r#"{"ConfigLoaded":{"failed":false}}"#),
            None
        );
        assert_eq!(screenshot_event("not json"), None);
    }
}
