mod clipboard;
mod watch;

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use magpie_core::{Library, Target};

/// Screenshot library and annotator for niri + Noctalia.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add a screenshot to the library, from a file or the clipboard.
    Add {
        #[arg(required_unless_present = "clipboard", conflicts_with = "clipboard")]
        path: Option<PathBuf>,
        /// Read an image/png from the clipboard instead of a file.
        #[arg(long)]
        clipboard: bool,
    },
    /// List shots and their variants, newest first.
    List {
        #[arg(long)]
        json: bool,
        /// Show at most this many shots.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Copy a shot version (`<id>` or `<id>@<version>`) to the clipboard.
    Copy { target: Target },
    /// Print the PNG path of a shot version.
    Path { target: Target },
    /// Open the annotation editor on a shot version.
    Edit { target: Target },
    /// Follow niri's event stream and add every captured screenshot.
    Watch {
        /// niri binary to run `msg --json event-stream` with.
        #[arg(long, default_value = "niri")]
        niri: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let lib = Library::open_default()?;
    match cli.command {
        Command::Add { path, clipboard } => {
            let id = match path {
                Some(path) if !clipboard => lib.add_file(&path)?.shot.id,
                _ => lib.add_png(&clipboard::paste_png()?)?.id,
            };
            println!("{id}");
        }
        Command::List { json, limit } => {
            let shots = lib.shots()?;
            let views: Vec<_> = shots
                .iter()
                .take(limit.unwrap_or(usize::MAX))
                .map(|s| lib.view(s))
                .collect();
            if json {
                println!("{}", serde_json::to_string(&views)?);
            } else {
                for v in &views {
                    println!(
                        "{}  {}  {}",
                        v.id,
                        v.taken_at.format("%Y-%m-%d %H:%M:%S"),
                        v.original.display()
                    );
                    for variant in &v.variants {
                        println!("    {} (from {})", variant.id, variant.parent);
                    }
                }
            }
        }
        Command::Copy { target } => clipboard::copy_png(&lib.resolve(&target)?)?,
        Command::Path { target } => println!("{}", lib.resolve(&target)?.display()),
        Command::Edit { target } => {
            lib.resolve(&target)?;
            bail!("the editor is not built yet");
        }
        Command::Watch { niri } => watch::run(&lib, &niri)?,
    }
    Ok(())
}
