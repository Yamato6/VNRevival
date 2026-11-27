# VNRevival

A modern, cross-platform runtime for **old visual novels** whose original
engines no longer run well on current systems. Like ScummVM does for
adventure games, VNRevival reads the data files of **your own legal copy**
and plays them on Windows, Linux and macOS.

VNRevival ships **no game data**.

## Supported engines

| Engine | Games | Status |
|---|---|---|
| OverflowADV2 (`adv2`) | Pure Mail (2001) | Boots and plays on the original scripts; many engine functions still missing |

## How it fits together

```
VNTools-rs (format crates)      VNRevival
┌──────────────────────┐        ┌───────────────────────────────────────┐
│ overflow-adv2        │──────▶ │ vnr-adv2   backend: engine → Session   │
│   packs, LZSS, ...   │        │ vnr-core   runtime, interface, Backend │
└──────────────────────┘        │ vnrevival  the executable              │
                                └───────────────────────────────────────┘
```

* **[VNTools-rs](https://github.com/Yamato6/VNTools-rs)** holds the
  readers for each engine's file formats, plus extraction tools and the
  reverse-engineering notes. Formats are figured out there first.
* **`vnr-core`** is engine-independent: the [`Backend`] interface, the
  shared runtime (a software compositor, a mixer for 44.1 kHz stereo
  clips with loops, Japanese text with system fonts, input events) and
  the **universal interface**, `Shell`: the main menu, pause menu, save
  slots, message history and settings, the same for every game. A game
  is a `Session` that only plays itself; the shell shows its screen
  scaled inside a 1280 × 720 interface. Nothing here knows about windows
  or devices, so it runs headless in tests.
* **`vnrevival`** is the executable. It drives the shell with SDL2
  (window, scaling, vsync, audio, input) or without any device for
  screenshots.
* **`vnr-<engine>`** crates are backends: they use the format crates to
  turn an original engine's scripts and assets into a `Session`.

Adding a new engine means adding its format crate to VNTools-rs and a
backend crate here; the runtime and the interface stay the same.

[`Backend`]: crates/vnr-core/src/lib.rs

## Usage

```sh
cargo run --release -- engines             # list supported engines
cargo run --release -- detect "<game dir>" # identify a game install
cargo run --release -- run "<game dir>"    # play
```

`run` options:

| Option | Meaning |
|---|---|
| `--script NAME` | start from another script instead of the normal entry point |
| `--font FILE` | font to use instead of the system's Japanese fonts |
| `--scale N` | initial window size, 1–3 times 1280 × 720 |
| `--new-game` | skip VNRevival's menu and start a new game |
| `--saves DIR` | folder for saved games instead of the default one |
| `--frames N` | run without a window for N frames (60 per second) |
| `--screenshot FILE` | with `--frames`: save the last frame as PNG |
| `--click-every N` | with `--frames`: click every N frames |
| `--click-at X,Y` | where those clicks land, in 1280 × 720 interface pixels (default `0,0`) |

A game opens on VNRevival's main menu (new game, load, extras,
settings, quit); the settings can switch to the game's own title screen
instead.

In a game: click, Enter or Space advance the text, Ctrl skips, the middle
button hides the message window. Escape or right-click open the pause
menu (save, load, history, settings, main menu, quit). Shortcuts: F2
saves, F3 loads, F4 opens the settings, the wheel up or Page Up opens the
message history (click a marked line to hear its voice again). Escape or
right-click close any screen; Alt+Enter toggles full screen. The window
can be resized; the picture keeps its aspect ratio.

**Pure Mail today:** the game boots like the original (`pmail.obj` +
`start.obj`) and runs its compiled scripts on a reimplementation of the
engine's script machine: variables, expressions, calls into the shared
functions of `pmail.obj`, branches and script changes all work (an
automated walk covers the whole game without a machine error). Implemented
on the engine side: the title menu, messages with speaker colours and
voices, choices, backgrounds, character sprites, colour fills,
transitions with the original timings (cross-fade and the "morning light"
wipe), waits, music, sound effects and voices, and the in-game PC:
the chat client, the mailer, letters, text labels, the taskbar clock,
pop-up animations and the opening movie (Indeo 5, decoded by VNTools-rs;
a click ends it). "New game" in VNRevival's menu runs the game's
start-up unseen and answers its title; with the original title screen,
its load button opens VNRevival's load screen.
Still missing: several effects; engine functions that are not implemented are skipped and
reported once on the console. The PC's layout comes from the game's own
window graphics and the engine's handlers, but fonts and colours inside
the windows are approximations.

### Saved games

There are 10 slots per game. You can save whenever the game waits for
you (a message, a choice, a chat line or an open mail). Saves are JSON
files in
`%APPDATA%\VNRevival\saves\<game>` on Windows,
`~/Library/Application Support/VNRevival/saves/<game>` on macOS and
`~/.local/share/vnrevival/saves/<game>` elsewhere (or under `--saves`).
They are VNRevival's own format: saves made with the original game are
not read.

### Extras

The extras screen has three tabs:

* **CG gallery**: every event picture, as thumbnails; a picture opens
  full screen and a click (or →) shows its next variant.
* **Music**: every track; click one to play it, again to stop.
* **Scenes**: the game's scene replay list; a scene plays from its start
  and returns to the list when it ends.

Only what you have seen or heard is open, unless "Unlock all extras" is
on in the settings. For Pure Mail, the pictures and scenes use the same
records as the original (`system.sav`, imported from the original's
`SAVE/SAVEFL99.SAV` the first time), so progress made with the original
counts; music heard is VNRevival's own record (`music.json`). The first
scene is the opening movie.

### Settings

The settings screen (F4) has three tabs: **System** (interface language:
English, Spanish or Japanese; what to show at start; full screen; unlock
all extras), **Sound** (master, music, sound effect and voice volumes)
and **Text** (text speed, message window opacity). They apply to every
game and are kept in `settings.json` in the user data folder
(`%APPDATA%\VNRevival` on Windows, `~/Library/Application
Support/VNRevival` on macOS, `~/.local/share/vnrevival` elsewhere).

Each interface language can be changed or completed with a
`lang/<code>.json` file in that folder (`en`, `es`, `ja`) holding
`{"key": "text"}` pairs; the keys are listed in `crates/vnr-core/src/lang.rs`.

Options that belong to one game are kept with its saves: for Pure Mail,
the hero's names in `options.json`.

### Fonts

The originals use Windows fonts (ＭＳ Ｐ明朝, ＭＳ Ｐゴシック). VNRevival
looks for them, or a close substitute, where each system installs them:
MS/Yu/BIZ UD fonts on Windows, IPA or Noto CJK on Linux (`fonts-ipafont`,
`fonts-noto-cjk`), Hiragino on macOS. Use `--font` to pick a file.

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
points at the install, e.g. `ADV2_GAME_DIR` for Pure Mail:

```sh
ADV2_GAME_DIR="/path/to/PMAILDVD" cargo test
```

### Working on VNTools-rs at the same time

The format crates are a git dependency. To use a local checkout instead,
create `.cargo/config.toml` (ignored by git) with:

```toml
[patch."https://github.com/Yamato6/VNTools-rs"]
overflow-adv2 = { path = "../VNTools-rs/overflow-adv2" }
indeo = { path = "../VNTools-rs/indeo" }
```

## Roadmap

1. ✅ OverflowADV2 formats in VNTools-rs: images, audio, script bytecode.
2. ✅ Runtime core: window, rendering and input on SDL2, Japanese text,
   audio mixing, headless runs with screenshots.
3. Pure Mail playable. Done: script interpreter (variables, expressions,
   `pmail.obj` functions, the engine's `sys` calls), choices, the in-game
   PC, the opening movie. To do: the remaining engine functions and
   effects.
4. Universal interface. Done: main menu, pause menu, save slots, history,
   settings, interface languages, CG gallery, music room, scene replay.
   To do: a game library with a folder picker, text input for names,
   a user manual.
5. Mods and translations.
6. More engines.

## License

[MIT](LICENSE).

Game names and trademarks belong to their owners. This project is not
affiliated with them.
