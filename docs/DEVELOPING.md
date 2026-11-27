# Developing VNRevival

How VNRevival is put together, how to build and test it, and how
releases are made. For playing, see the [user manual](MANUAL.md).

* [How it fits together](#how-it-fits-together)
* [Building and testing](#building-and-testing)
* [Command line](#command-line)
* [Pure Mail: what is implemented](#pure-mail-what-is-implemented)
* [Release packages](#release-packages)
* [Working on VNTools-rs at the same time](#working-on-vntools-rs-at-the-same-time)

## How it fits together

```
VNTools-rs (format crates)      VNRevival
┌──────────────────────┐        ┌───────────────────────────────────────┐
│ overflow-adv2        │──────▶ │ vnr-adv2   backend: engine → Session   │
│   packs, LZSS, ...   │        │ vnr-core   runtime, interface, Backend │
│ indeo (video)        │        │ vnrevival  the executable              │
└──────────────────────┘        └───────────────────────────────────────┘
```

* **[VNTools-rs](https://github.com/Yamato6/VNTools-rs)** holds the
  readers for each engine's file formats, plus extraction tools and the
  reverse-engineering notes. Formats are figured out there first.
* **`vnr-core`** is engine-independent: the [`Backend`] interface, the
  shared runtime (a software compositor, a mixer for 44.1 kHz stereo
  clips with loops, Japanese text with installed fonts, input events)
  and the **universal interface**, `Shell`: the game library, main menu,
  pause menu, save slots, message history, settings and extras, the same
  for every game. A game is a `Session` that only plays itself; the shell
  shows its screen scaled inside a 1280 × 720 interface. Nothing here
  knows about windows or devices, so it runs headless in tests.
* **`vnrevival`** is the executable. It drives the shell with SDL2
  (window, scaling, vsync, audio, input) or without any device for
  screenshots.
* **`vnr-<engine>`** crates are backends: they use the format crates to
  turn an original engine's scripts and assets into a `Session`.

Adding a new engine means adding its format crate to VNTools-rs and a
backend crate here; the runtime and the interface stay the same.

[`Backend`]: ../crates/vnr-core/src/lib.rs

## Building and testing

Requires Rust 1.85 or newer, plus **CMake** and a C compiler: SDL2 is
built from source and linked statically (the `bundled` and `static-link`
features of the `sdl2` crate), so the result has no SDL DLL to ship.

* **Windows:** Visual Studio or its Build Tools. If `cmake` is not on
  `PATH`, point the `CMAKE` environment variable at the one Visual Studio
  ships (`Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe`),
  for example in `.cargo/config.toml` under `[env]`.
* **Linux:** `cmake` and the development headers SDL needs for a window
  and sound, e.g. on Debian/Ubuntu
  `sudo apt install cmake libx11-dev libxext-dev libxrandr-dev libxcursor-dev libxi-dev libxss-dev libwayland-dev libxkbcommon-dev libasound2-dev libpulse-dev`.
* **macOS:** `brew install cmake`.

```sh
cargo build --release
cargo test
```

Tests that need real game data are skipped unless an environment variable
points at the install, e.g. `ADV2_GAME_DIR` for Pure Mail. With
`VNR_SHOT_DIR` set, some of them also save screenshots there (the
`interface_tour` test goes through every screen of the interface):

```sh
ADV2_GAME_DIR="/path/to/PMAILDVD" cargo test
```

CI runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`
and `cargo test` on Windows, Linux and macOS.

A build of your own uses the installed fonts: install Noto Sans JP from
Google Fonts on Windows and macOS, or `fonts-noto-cjk` on Linux.
VNRevival also looks for fonts in `fonts` next to the executable,
`../Resources/fonts` (macOS app) and `../share/vnrevival/fonts`
(AppImage), after the installed ones.

## Command line

```sh
cargo run --release -- engines             # list supported engines
cargo run --release -- detect "<game dir>" # identify a game install
cargo run --release -- run                 # the game library
cargo run --release -- run "<game dir>"    # play a game directly
```

Without a command, `run` is assumed: opening the executable from a file
manager shows the library. On Windows it is a GUI program (no console
window); started from a terminal it still prints there, but the
terminal does not wait for it (in PowerShell, `Start-Process -Wait
-NoNewWindow` does). Errors are also shown in a message box.

`run` options:

| Option | Meaning |
|---|---|
| `--script NAME` | start from another script instead of the normal entry point |
| `--font FILE` | font file to use instead of the font settings |
| `--scale N` | initial window size, 1–3 times 1280 × 720 |
| `--new-game` | skip VNRevival's menu and start a new game |
| `--saves DIR` | folder for saved games instead of the default one |
| `--frames N` | run without a window for N frames (60 per second) |
| `--screenshot FILE` | with `--frames`: save the last frame as PNG |
| `--click-every N` | with `--frames`: click every N frames |
| `--click-at X,Y` | where those clicks land, in 1280 × 720 interface pixels (default `0,0`) |

Engine functions that are not implemented yet are skipped and reported
once on the console.

## Pure Mail: what is implemented

The game boots like the original (`pmail.obj` + `start.obj`) and runs
its compiled scripts on a reimplementation of the engine's script
machine: variables, expressions, calls into the shared functions of
`pmail.obj`, branches and script changes all work (an automated walk
covers the whole game without a machine error).

Implemented on the engine side: the title menu, messages with speaker
colours and voices, choices, backgrounds, character sprites, colour
fills, transitions with the original timings (cross-fade and the
"morning light" wipe), waits, music, sound effects and voices, and the
in-game PC: the chat client, the mailer, letters, text labels, the
taskbar clock, pop-up animations, the opening movie (Indeo 5, decoded by
VNTools-rs), music and sound fade-outs, the calendar's voices, and each
ending's staff roll with the heroine's memories. "New game" in
VNRevival's menu runs the game's start-up unseen and answers its title;
with the original title screen, its load button opens VNRevival's load
screen.

The extras use the same records as the original (`system.sav`, imported
from the original's `SAVE/SAVEFL99.SAV` the first time), so progress
made with the original counts; music heard is VNRevival's own record
(`music.json`). Saves are VNRevival's own JSON format: saves made with
the original game are not read.

Still missing: several effects and engine functions. The PC's layout
comes from the game's own window graphics and the engine's handlers,
but fonts and colours inside the windows are approximations.

## Release packages

Pushing a tag `vX.Y.Z` that matches the version in `Cargo.toml` runs
[`release.yml`](../.github/workflows/release.yml): it builds and packs
VNRevival on each system with [`packaging/package.sh`](../packaging/package.sh)
and creates a **draft** release with the packages, to be checked and
published by hand. Running the workflow by hand (*Actions → Release →
Run workflow*) builds the packages as artifacts only.

| Package | Contents |
|---|---|
| `VNRevival-<v>-windows-x64.zip` | `VNRevival.exe` (C runtime linked in, icon and version embedded), `fonts/` |
| `VNRevival-<v>-macos.dmg` | `VNRevival.app`, one binary for Intel and Apple Silicon, macOS 11+, ad-hoc signed |
| `VNRevival-<v>-linux-x64.AppImage` | the program, its font, icon and desktop entry, built on Ubuntu 22.04 |
| `VNRevival-<v>-linux-x64.tar.gz` | the same as a plain folder |

Each also holds the license, the manual and Noto Sans JP with its
license (`OFL.txt`), downloaded from a pinned commit of
[google/fonts](https://github.com/google/fonts) and checked against a
SHA-256.

The icon and the banner are drawn by
[`assets/icon/make_icon.py`](../assets/icon/make_icon.py) (Pillow, with
Noto Serif JP and Noto Sans JP installed): run it after changing it and
commit the images it writes.

To pack by hand, build in release mode and run
`packaging/package.sh windows|linux|macos <binary>` (Git Bash on
Windows); the package goes to `dist/`.

## Working on VNTools-rs at the same time

The format crates are a git dependency. To use a local checkout instead,
create `.cargo/config.toml` (ignored by git) with:

```toml
[patch."https://github.com/Yamato6/VNTools-rs"]
overflow-adv2 = { path = "../VNTools-rs/overflow-adv2" }
indeo = { path = "../VNTools-rs/indeo" }
```

Comment the patch out again before committing, so that `Cargo.lock`
points at a VNTools-rs commit on GitHub: CI cannot see the local path.
