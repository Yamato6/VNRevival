# VNRevival

A modern, cross-platform runtime for **old visual novels** whose original
engines no longer run well on current systems. Like ScummVM does for
adventure games, VNRevival reads the data files of **your own legal copy**
and plays them on Windows, Linux and macOS.

VNRevival ships **no game data**.

## Supported engines

| Engine | Games | Status |
|---|---|---|
| OverflowADV2 (`adv2`) | Pure Mail (2001) | Detection only; runtime not started |

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
* **`vnr-core`** is engine-independent: the [`Backend`] interface and,
  as it gets written, the shared runtime (window, rendering, text, audio,
  input, saves, settings).
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
```

## Building and testing

Requires Rust 1.85 or newer.

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

1. OverflowADV2 formats in VNTools-rs: images, audio, script bytecode.
2. Runtime core: window, rendering and input on SDL, text with
   Japanese fonts, audio mixing.
3. Pure Mail playable: script interpreter, choices, save/load, settings.
4. Video (Indeo 5 movies), mods and translations.
5. More engines.

## License

[MIT](LICENSE).

Game names and trademarks belong to their owners. This project is not
affiliated with them.
