// atlasfetch — centered ASCII art with powerline panels
//
// Design: The binary has two modes. The default mode prints system info
// instantly. The `setup` subcommand launches a TUI configurator. Both share
// the same rendering engine so the preview in setup is identical to real
// terminal output.

mod ascii;
mod cli;
mod config;
mod info;
mod layout;
mod render;
mod theme;
mod tui;

use clap::Parser;
use color_eyre::Result;

fn main() -> Result<()> {
    color_eyre::install()?;

    let args = cli::Args::parse();

    // --list-presets: print and exit
    if args.list_presets {
        let themes = theme::all_themes();
        println!("Available presets:");
        for t in &themes {
            let swatch: String = t
                .colors
                .iter()
                .map(|c| format!("\x1b[48;2;{};{};{}m  \x1b[0m", c.r, c.g, c.b))
                .collect();
            println!("  {:20} {}", t.name, swatch);
        }
        return Ok(());
    }

    // --preset: apply and exit
    if let Some(ref name) = args.preset {
        let themes = theme::all_themes();
        if let Some(t) = themes.iter().find(|t| t.name == *name) {
            let mut cfg = config::Config::load()?;
            cfg.logo.colors = t.colors.clone();
            cfg.save()?;
            println!("Preset \"{}\" applied.", name);
        } else {
            eprintln!("Preset \"{}\" not found. Use --list-presets.", name);
        }
        return Ok(());
    }

    // setup: launch TUI configurator
    if args.setup {
        let mut cfg = config::Config::load()?;
        tui::run(&mut cfg)?;
        return Ok(());
    }

    // default: print fetch output
    ascii::ensure_logos()?;
    let cfg = config::Config::load()?;
    let info = info::collect()?;
    let ascii_art = ascii::load(&cfg)?;
    let output = render::render(&cfg, &info, &ascii_art)?;
    print!("{}", output);
    Ok(())
}
