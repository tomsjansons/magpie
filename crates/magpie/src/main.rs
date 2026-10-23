use clap::{Parser, Subcommand};

/// Screenshot library and annotator for niri + Noctalia.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add a screenshot file to the library.
    Add { path: Option<String>, #[arg(long)] clipboard: bool },
    /// List shots and their variants as JSON.
    List { #[arg(long)] json: bool },
    /// Copy a shot or variant (`<id>[@variant]`) to the clipboard.
    Copy { target: String },
    /// Open the annotation editor on a shot or variant.
    Edit { target: String },
    /// Follow niri's event stream and add every captured screenshot.
    Watch,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        _ => anyhow::bail!("not implemented yet"),
    }
}
