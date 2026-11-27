//! `vnrevival`: play old visual novels from your own copy.
//!
//! `run` opens the game library, or a game folder directly, in a window
//! (SDL2), or runs without one for a fixed number of frames and saves a
//! screenshot (`--frames`). Every game is shown inside VNRevival's own
//! interface ([`vnr_core::shell`]).

mod sdl;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use vnr_core::shell::{Shell, ShellOptions};
use vnr_core::{
    Backend, GameInfo, InputEvent, LaunchOptions, Launcher, Mixer, MouseButton, Session, Start,
};

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
    /// Play: open the game library, or a game folder directly.
    Run {
        /// Game install folder (it is added to the library); without it
        /// the library opens.
        game: Option<PathBuf>,
        /// Start from this script instead of the default (engine-specific).
        #[arg(long)]
        script: Option<String>,
        /// Font file to use instead of the system's Japanese fonts.
        #[arg(long)]
        font: Option<PathBuf>,
        /// Folder for saved games (default: a folder per game in the
        /// user's data directory).
        #[arg(long)]
        saves: Option<PathBuf>,
        /// Initial window size as a multiple of the interface (1280 × 720).
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=3))]
        scale: u32,
        /// Skip VNRevival's menu and start a new game at once.
        #[arg(long)]
        new_game: bool,
        /// Run without a window for this many frames (60 per second).
        #[arg(long)]
        frames: Option<u64>,
        /// With --frames: save the last frame as a PNG.
        #[arg(long, requires = "frames")]
        screenshot: Option<PathBuf>,
        /// With --frames: click every N frames to advance the text.
        #[arg(long, requires = "frames")]
        click_every: Option<u64>,
        /// Where --click-every clicks, as X,Y in interface pixels
        /// (1280 × 720).
        #[arg(long, value_parser = parse_point, default_value = "0,0")]
        click_at: (i32, i32),
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
            saves,
            scale,
            new_game,
            frames,
            screenshot,
            click_every,
            click_at,
        } => {
            let options = LaunchOptions {
                entry: script,
                font,
                saves,
            };
            let clicks = click_every.map(|n| (n, click_at));
            let shell = match &game {
                Some(game) => shell(game, &options, new_game),
                None => launcher_shell(&options),
            };
            let result = shell.and_then(|mut shell| match frames {
                Some(frames) => run_headless(&mut shell, frames, clicks, screenshot.as_deref()),
                None => sdl::run(&mut shell, &Mixer::new(MIXER_CHANNELS), scale),
            });
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

/// Detects the game in `dir` and opens it with its backend.
fn open(dir: &Path, options: &LaunchOptions) -> Result<(GameInfo, Box<dyn Session>), String> {
    for backend in backends() {
        let found = backend
            .detect(dir)
            .map_err(|e| format!("{}: {e}", backend.name()))?;
        if let Some(info) = found {
            let session = backend
                .open(&info, options)
                .map_err(|e| format!("{}: {e}", info.title))?;
            return Ok((info, session));
        }
    }
    Err(format!("no supported game found in {}", dir.display()))
}

/// Every backend behind the library.
struct Backends {
    options: LaunchOptions,
}

impl Launcher for Backends {
    fn detect(&self, dir: &Path) -> Result<Option<GameInfo>, String> {
        detect(dir)
    }

    fn open(&self, game: &GameInfo) -> Result<Box<dyn Session>, String> {
        let backend = backends()
            .into_iter()
            .find(|b| b.id() == game.engine)
            .ok_or_else(|| format!("no backend {}", game.engine))?;
        backend
            .open(game, &self.options)
            .map_err(|e| format!("{}: {e}", game.title))
    }
}

/// The game in `dir` inside the universal interface (with the library
/// behind "Change game").
fn shell(dir: &Path, options: &LaunchOptions, new_game: bool) -> Result<Shell, String> {
    let (info, session) = open(dir, options)?;
    let font = vnr_core::shell::load_font(options.font.as_deref())?;
    let saves = options.saves.clone().or_else(|| vnr_core::save_dir(&info));
    let mut shell = Shell::new(
        session,
        font,
        ShellOptions {
            data_dir: vnr_core::user_data_dir(),
            saves,
            font_file: options.font.clone(),
        },
    );
    shell.attach_launcher(
        Box::new(Backends {
            options: options.clone(),
        }),
        &info,
    );
    if new_game {
        shell.begin(Start::NewGame);
    }
    Ok(shell)
}

/// The game library.
fn launcher_shell(options: &LaunchOptions) -> Result<Shell, String> {
    let font = vnr_core::shell::load_font(options.font.as_deref())?;
    Ok(Shell::launcher(
        Box::new(Backends {
            options: options.clone(),
        }),
        font,
        ShellOptions {
            data_dir: vnr_core::user_data_dir(),
            saves: None,
            font_file: options.font.clone(),
        },
    ))
}

fn run_headless(
    shell: &mut Shell,
    frames: u64,
    clicks: Option<(u64, (i32, i32))>,
    screenshot: Option<&Path>,
) -> Result<(), String> {
    let mixer = Mixer::new(MIXER_CHANNELS);
    let script: Vec<(u64, InputEvent)> = match clicks {
        Some((n, (x, y))) if n > 0 => {
            let click = InputEvent::MouseDown {
                button: MouseButton::Left,
                x,
                y,
            };
            (n..frames)
                .step_by(n as usize)
                .map(|f| (f, click.clone()))
                .collect()
        }
        _ => Vec::new(),
    };
    let canvas = vnr_core::run_headless(shell, &mixer, frames, &script);
    if let Some(path) = screenshot {
        canvas
            .save_png(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        println!("saved {}", path.display());
    }
    Ok(())
}

/// Parses `X,Y`.
fn parse_point(text: &str) -> Result<(i32, i32), String> {
    let (x, y) = text
        .split_once(',')
        .ok_or_else(|| format!("expected X,Y, got {text:?}"))?;
    let parse = |v: &str| v.trim().parse::<i32>().map_err(|e| format!("{v:?}: {e}"));
    Ok((parse(x)?, parse(y)?))
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

#[cfg(test)]
mod tests {
    use super::*;
    use vnr_core::{Canvas, Key};

    /// Goes through the interface with Pure Mail: menu, a new game, the
    /// pause menu, settings, history and saves (needs `ADV2_GAME_DIR`;
    /// screenshots go to `$VNR_SHOT_DIR`).
    #[test]
    fn interface_tour() {
        let Some(dir) = std::env::var_os("ADV2_GAME_DIR") else {
            eprintln!("ADV2_GAME_DIR not set, skipping");
            return;
        };
        let scratch = std::env::temp_dir().join(format!("vnr-tour-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        let options = LaunchOptions {
            saves: Some(scratch.join("saves")),
            ..LaunchOptions::default()
        };
        let (info, session) = open(Path::new(&dir), &options).unwrap();
        // Show every extra, seen or not.
        std::fs::create_dir_all(scratch.join("data")).unwrap();
        std::fs::write(
            scratch.join("data/settings.json"),
            r#"{"unlock_all": true}"#,
        )
        .unwrap();
        let Ok(font) = vnr_core::shell::load_font(None) else {
            eprintln!("no font, skipping");
            return;
        };
        let mut shell = Shell::new(
            session,
            font,
            ShellOptions {
                data_dir: Some(scratch.join("data")),
                saves: options.saves.clone(),
                font_file: None,
            },
        );
        shell.attach_launcher(
            Box::new(Backends {
                options: options.clone(),
            }),
            &info,
        );
        let mixer = Mixer::new(MIXER_CHANNELS);
        let shots = std::env::var_os("VNR_SHOT_DIR");
        let shoot = |canvas: &Canvas, name: &str| {
            if let Some(out) = &shots {
                canvas
                    .save_png(&Path::new(out).join(format!("ui-{name}.png")))
                    .unwrap();
            }
        };
        let click = |x, y| InputEvent::MouseDown {
            button: MouseButton::Left,
            x,
            y,
        };
        let key = InputEvent::KeyDown;

        let canvas = vnr_core::run_headless(&mut shell, &mixer, 3, &[]);
        shoot(&canvas, "menu");
        assert!(!shell.in_game());
        vnr_core::run_headless(&mut shell, &mixer, 1, &[(0, click(600, 260))]);
        assert!(shell.in_game());
        // A new game asks for the hero's name; an input method composes
        // a word in the first field.
        let mut frames = 0;
        while shell.text_input().is_none() {
            vnr_core::run_headless(&mut shell, &mixer, 1, &[]);
            frames += 1;
            assert!(frames < 2000, "the names were not asked");
        }
        let composing = InputEvent::Composition("けい".into());
        let canvas = vnr_core::run_headless(&mut shell, &mixer, 1, &[(0, composing)]);
        shoot(&canvas, "names");
        let done = [
            (0, InputEvent::Composition(String::new())),
            (1, key(Key::Escape)),
        ];
        vnr_core::run_headless(&mut shell, &mixer, 2, &done);
        assert!(shell.text_input().is_none() && shell.in_game());
        let advance: Vec<_> = (10..1500)
            .step_by(20)
            .map(|f| (f, click(640, 360)))
            .collect();
        let canvas = vnr_core::run_headless(&mut shell, &mixer, 1500, &advance);
        shoot(&canvas, "game");
        assert!(shell.session().can_save());

        for (name, open) in [
            ("pause", key(Key::Escape)),
            ("settings", key(Key::F(4))),
            ("history", InputEvent::Wheel(1)),
            ("save", key(Key::F(2))),
        ] {
            let canvas = vnr_core::run_headless(&mut shell, &mixer, 2, &[(0, open)]);
            shoot(&canvas, name);
            assert!(!shell.in_game(), "{name} did not open");
            vnr_core::run_headless(&mut shell, &mixer, 1, &[(0, key(Key::Escape))]);
            assert!(shell.in_game(), "{name} did not close");
        }

        // The text and reading settings.
        vnr_core::run_headless(&mut shell, &mixer, 1, &[(0, key(Key::F(4)))]);
        for (name, x) in [
            ("settings-text", 640),
            ("settings-reading", 810),
            ("settings-game", 980),
        ] {
            let canvas = vnr_core::run_headless(&mut shell, &mixer, 2, &[(0, click(x, 160))]);
            shoot(&canvas, name);
        }
        vnr_core::run_headless(&mut shell, &mixer, 1, &[(0, key(Key::Escape))]);
        // Auto mode, shown over the game.
        let a = key(Key::Char('a'));
        let canvas = vnr_core::run_headless(&mut shell, &mixer, 2, &[(0, a.clone())]);
        assert!(shell.auto());
        shoot(&canvas, "auto");
        vnr_core::run_headless(&mut shell, &mixer, 1, &[(0, a)]);

        // Main menu (pause, 7th item), then the extras.
        vnr_core::run_headless(&mut shell, &mixer, 1, &[(0, key(Key::Escape))]);
        vnr_core::run_headless(&mut shell, &mixer, 1, &[(0, click(600, 530))]);
        assert!(!shell.in_game());
        for (name, x, y) in [
            ("gallery", 600, 390),
            ("viewer", 150, 250),
            ("viewer-next", 640, 360),
        ] {
            let canvas = vnr_core::run_headless(&mut shell, &mixer, 2, &[(0, click(x, y))]);
            shoot(&canvas, name);
        }
        vnr_core::run_headless(&mut shell, &mixer, 1, &[(0, key(Key::Escape))]);
        for (name, x) in [("music", 500), ("scenes", 700)] {
            let canvas = vnr_core::run_headless(&mut shell, &mixer, 2, &[(0, click(x, 160))]);
            shoot(&canvas, name);
        }
        // Replay the second scene (the first is the opening movie).
        let canvas = vnr_core::run_headless(&mut shell, &mixer, 300, &[(0, click(334, 250))]);
        assert!(shell.in_game());
        shoot(&canvas, "replay");

        // Leave the replay, then "Change game": the library, and the
        // folder picker where the game was found.
        vnr_core::run_headless(&mut shell, &mixer, 1, &[(0, key(Key::Escape))]);
        vnr_core::run_headless(&mut shell, &mixer, 1, &[(0, click(600, 530))]);
        vnr_core::run_headless(&mut shell, &mixer, 1, &[(0, key(Key::Escape))]);
        let canvas = vnr_core::run_headless(&mut shell, &mixer, 2, &[(0, click(600, 510))]);
        assert!(!shell.in_game());
        assert_eq!(shell.session().title(), "VNRevival");
        shoot(&canvas, "library");
        let canvas = vnr_core::run_headless(&mut shell, &mixer, 2, &[(0, click(800, 70))]);
        shoot(&canvas, "picker");
        let _ = std::fs::remove_dir_all(scratch);
    }
}
