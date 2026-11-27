//! `vnrevival`: play old visual novels from your own copy.
//!
//! `run` opens a game in a window (SDL2), or runs it without one for a
//! fixed number of frames and saves a screenshot (`--frames`).

mod sdl;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use vnr_core::{Backend, GameInfo, InputEvent, LaunchOptions, Mixer, MouseButton};

/// Mixer channels opened for every game.
const MIXER_CHANNELS: usize = 8;

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
    /// Play a game.
    Run {
        /// Game install folder.
        game: PathBuf,
        /// Start from this script instead of the default (engine-specific).
        #[arg(long)]
        script: Option<String>,
        /// Font file to use instead of the system's Japanese fonts.
        #[arg(long)]
        font: Option<PathBuf>,
        /// Initial window size as a multiple of the game resolution.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=4))]
        scale: u32,
        /// Run without a window for this many frames (60 per second).
        #[arg(long)]
        frames: Option<u64>,
        /// With --frames: save the last frame as a PNG.
        #[arg(long, requires = "frames")]
        screenshot: Option<PathBuf>,
        /// With --frames: click every N frames to advance the text.
        #[arg(long, requires = "frames")]
        click_every: Option<u64>,
    },
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
        Command::Run {
            game,
            script,
            font,
            scale,
            frames,
            screenshot,
            click_every,
        } => {
            let options = LaunchOptions {
                entry: script,
                font,
            };
            let result = match frames {
                Some(frames) => {
                    run_headless(&game, &options, frames, click_every, screenshot.as_deref())
                }
                None => run_window(&game, &options, scale),
            };
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    eprintln!("error: {message}");
                    ExitCode::FAILURE
                }
            }
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

/// Detects the game in `dir` and starts it with its backend.
fn launch(dir: &Path, options: &LaunchOptions) -> Result<Box<dyn vnr_core::App>, String> {
    for backend in backends() {
        let found = backend
            .detect(dir)
            .map_err(|e| format!("{}: {e}", backend.name()))?;
        if let Some(info) = found {
            return backend
                .launch(&info, options)
                .map_err(|e| format!("{}: {e}", info.title));
        }
    }
    Err(format!("no supported game found in {}", dir.display()))
}

fn run_window(dir: &Path, options: &LaunchOptions, scale: u32) -> Result<(), String> {
    let mut app = launch(dir, options)?;
    let mixer = Mixer::new(MIXER_CHANNELS);
    sdl::run(app.as_mut(), &mixer, scale)
}

fn run_headless(
    dir: &Path,
    options: &LaunchOptions,
    frames: u64,
    click_every: Option<u64>,
    screenshot: Option<&Path>,
) -> Result<(), String> {
    let mut app = launch(dir, options)?;
    let mixer = Mixer::new(MIXER_CHANNELS);
    let click = InputEvent::MouseDown {
        button: MouseButton::Left,
        x: 0,
        y: 0,
    };
    let script: Vec<(u64, InputEvent)> = match click_every {
        Some(n) if n > 0 => (n..frames)
            .step_by(n as usize)
            .map(|f| (f, click))
            .collect(),
        _ => Vec::new(),
    };
    let canvas = vnr_core::run_headless(app.as_mut(), &mixer, frames, &script);
    if let Some(path) = screenshot {
        canvas
            .save_png(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        println!("saved {}", path.display());
    }
    Ok(())
}

fn detect(dir: &Path) -> Result<Option<GameInfo>, String> {
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
