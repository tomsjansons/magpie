//! Follow `niri msg --json event-stream` and import every screenshot, and
//! report library changes made by anything else (the editor saving a variant).

use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use magpie_core::Library;
use notify::{RecursiveMode, Watcher};
use serde::Serialize;
use serde_json::Value;

use magpie_core::clipboard;

/// Library writes arrive as bursts of file events; one `changed` per burst.
const SETTLE: Duration = Duration::from_millis(150);

/// One line of `magpie watch` output.
#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Out {
    Ready,
    Added { id: String, new: bool },
    Changed,
    Error { message: String },
}

enum Msg {
    Niri(String),
    NiriEnded(String),
    Library,
}

fn emit(out: &Out) {
    // Rust's stdout is line-buffered, so each event reaches the reader at once.
    println!("{}", serde_json::to_string(out).expect("event serializes"));
}

pub fn run(lib: &Library, niri: &str) -> Result<()> {
    let (tx, rx) = mpsc::channel();

    let mut command = Command::new(niri);
    command
        .args(["msg", "--json", "event-stream"])
        .stdout(Stdio::piped());
    // The plugin stops us with a plain kill; take niri's client down too
    // instead of leaving it to linger until its next write fails.
    // SAFETY: prctl is async-signal-safe and touches no Rust state.
    unsafe {
        command.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("running {niri} msg event-stream"))?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let niri_tx = tx.clone();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if niri_tx.send(Msg::Niri(line)).is_err() {
                return;
            }
        }
        let status = child
            .wait()
            .map(|s| s.to_string())
            .unwrap_or_else(|e| e.to_string());
        let _ = niri_tx.send(Msg::NiriEnded(status));
    });

    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|e| e.paths.iter().any(|p| is_meta(p))) {
            let _ = tx.send(Msg::Library);
        }
    })?;
    watcher.watch(&lib.root().join("shots"), RecursiveMode::Recursive)?;

    emit(&Out::Ready);
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Niri(line) => {
                if let Some(path) = screenshot_event(&line) {
                    emit(&import(lib, path));
                }
            }
            Msg::Library => {
                while let Ok(Msg::Library) = rx.recv_timeout(SETTLE) {}
                emit(&Out::Changed);
            }
            Msg::NiriEnded(status) => bail!("niri event stream ended: {status}"),
        }
    }
    bail!("watch channel closed")
}

fn import(lib: &Library, path: Option<String>) -> Out {
    let result = match path {
        Some(path) => lib.add_file(Path::new(&path)).map(|a| (a.shot.id, a.new)),
        None => clipboard::paste_png()
            .and_then(|png| lib.add_png(&png))
            .map(|shot| (shot.id, true)),
    };
    match result {
        Ok((id, new)) => Out::Added { id, new },
        Err(err) => Out::Error {
            message: format!("{err:#}"),
        },
    }
}

fn is_meta(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n == "meta.json")
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
