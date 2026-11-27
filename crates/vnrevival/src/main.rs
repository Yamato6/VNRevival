//! `vnrevival`: play old visual novels from your own copy.
//!
//! The runtime is not written yet; for now the binary detects which engine
//! and game a folder holds.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use vnr_core::{Backend, GameInfo};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Detect the engine and game in a folder.
    Detect {
        /// Game install folder.
        game: PathBuf,
    },
    /// List the supported engines.
    Engines,
}

/// Every backend, in detection order.
fn backends() -> Vec<Box<dyn Backend>> {
    vec![Box::new(vnr_adv2::Adv2)]
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Engines => {
            for backend in backends() {
                println!("{:<8} {}", backend.id(), backend.name());
            }
            ExitCode::SUCCESS
        }
        Command::Detect { game } => match detect(&game) {
            Ok(Some(info)) => {
                print_info(&info);
                ExitCode::SUCCESS
            }
            Ok(None) => {
                eprintln!("no supported game found in {}", game.display());
                ExitCode::FAILURE
            }
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::FAILURE
            }
        },
    }
}

fn detect(dir: &std::path::Path) -> Result<Option<GameInfo>, String> {
    if !dir.is_dir() {
        return Err(format!("{} is not a folder", dir.display()));
    }
    for backend in backends() {
        if let Some(info) = backend
            .detect(dir)
            .map_err(|e| format!("{}: {e}", backend.name()))?
        {
            return Ok(Some(info));
        }
    }
    Ok(None)
}

fn print_info(info: &GameInfo) {
    println!("title:  {}", info.title);
    println!("engine: {}", info.engine);
    println!("folder: {}", info.root.display());
    for (label, value) in &info.details {
        println!("  {label:<12} {value}");
    }
}
