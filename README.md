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
│ overflow-adv2        │──────▶ │ vnr-adv2   backend: engine → runtime   │
│   packs, LZSS, ...   │        │ vnr-core   shared runtime & Backend    │
└──────────────────────┘        │ vnrevival  the executable              │
                                └───────────────────────────────────────┘
```

* **[VNTools-rs](https://github.com/Yamato6/VNTools-rs)** holds the
  readers for each engine's file formats, plus extraction tools and the
  reverse-engineering notes. Formats are figured out there first.
* **`vnr-core`** is engine-independent: the [`Backend`] interface and the
  shared runtime: a software compositor (`Canvas`, `Sprite`), a mixer for
  44.1 kHz stereo clips with loops, Japanese text with system fonts, input
  events, and the `App` trait a game implements. It knows nothing about
  windows or devices, so it runs headless in tests.
* **`vnrevival`** is the executable. It drives an `App` with SDL2 (window,
  scaling, vsync, audio, input) or without any device for screenshots.
* **`vnr-<engine>`** crates are backends: they use the format crates to
  turn an original engine's scripts and assets into what the runtime
  needs.

Adding a new engine means adding its format crate to VNTools-rs and a
backend crate here; the runtime stays the same.

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
| `--scale N` | initial window size, 1–4 times the game resolution |
| `--frames N` | run without a window for N frames (60 per second) |
| `--screenshot FILE` | with `--frames`: save the last frame as PNG |
| `--click-every N` | with `--frames`: click every N frames |
| `--click-at X,Y` | where those clicks land (default `0,0`) |

In the window: click, Enter or Space advance the text, right-click hides
the message window, Ctrl skips, Escape quits, Alt+Enter toggles full
screen. The window can be resized; the picture keeps its aspect ratio.

**Pure Mail today:** the game boots like the original (`pmail.obj` +
`start.obj`) and runs its compiled scripts on a reimplementation of the
engine's script machine: variables, expressions, calls into the shared
functions of `pmail.obj`, branches and script changes all work (an
automated walk covers the whole game without a machine error). Implemented
on the engine side: the title menu, messages with speaker colours and
voices, choices, backgrounds, character sprites, colour fills,
cross-fades, waits, music, sound effects and voices. Still missing: the
in-game PC (chat and mail windows), save/load, the backlog, settings and
several effects; engine functions that are not implemented are skipped and
reported once on the console.

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
```

## Roadmap

1. ✅ OverflowADV2 formats in VNTools-rs: images, audio, script bytecode.
2. ✅ Runtime core: window, rendering and input on SDL2, Japanese text,
   audio mixing, headless runs with screenshots.
3. Pure Mail playable: script interpreter (variables, expressions,
   `pmail.obj` functions, the engine's `sys` calls), choices, save/load,
   settings.
4. Video (Indeo 5 movies), mods and translations.
5. More engines.

## License

[MIT](LICENSE).

Game names and trademarks belong to their owners. This project is not
affiliated with them.
