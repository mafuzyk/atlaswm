// CLI argument parsing via clap derive.
// Kept minimal: the subcommand model splits normal execution from setup.

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "atlasfetch", about = "Centered ASCII art with powerline panels", version)]
pub struct Args {
    /// Launch interactive setup TUI
    #[arg(short = 'i', long = "setup")]
    pub setup: bool,

    /// Apply a preset palette and exit
    #[arg(long = "preset")]
    pub preset: Option<String>,

    /// List available presets with color swatches
    #[arg(long = "list-presets")]
    pub list_presets: bool,
}
