# OpenADV2

An open reimplementation of **OverflowADV2**, the visual novel engine behind
early Overflow titles such as **Pure Mail** (2001). The goal is to play
those games on modern Windows, Linux and macOS using the data files from
your own legal copy.

OpenADV2 ships **no game data**. It only reads files from an install you
own.

## Status

Early research stage.

| Area | Status |
|---|---|
| Pack archives (`*.DAT`) | ✅ read, list and extract |
| LZSS compression | ✅ |
| Images | 🟡 `.dib` understood, other formats pending |
| Scripts (`.obj` bytecode) | ❓ not started |
| Audio (`.adp4` / `.adps`) | ❓ not started |
| Video (Indeo 5 AVI) | identified, playback not started |
| Engine runtime | ❓ not started |

Format notes: [docs/FORMATS.md](docs/FORMATS.md).

## Building

Requires Rust 1.85 or newer.

```sh
cargo build --release
```

## `adv2-tool`

```sh
# Summary of a pack
adv2-tool info  "<game>/PACK/GRAPHIC.DAT"

# List members (optionally filtered by name)
adv2-tool list  "<game>/PACK/SCRIPT.DAT" --filter start

# Extract members (decompressed; add --raw to keep them as stored)
adv2-tool extract "<game>/PACK/SCRIPT.DAT" --output extracted/script
```

## Tests

```sh
cargo test
```

Tests that need real game data are skipped unless `ADV2_GAME_DIR` points
at the install folder (the one that contains `PACK/`):

```sh
ADV2_GAME_DIR="/path/to/PMAILDVD" cargo test --release
```

## Project layout

```
crates/adv2-formats   library: readers for the engine's file formats
crates/adv2-tool      command-line inspector / extractor
docs/FORMATS.md       reverse-engineering notes
```

## Roadmap

1. Image formats (`.crgb`, `.char`, `.anim`, …) and a PNG export.
2. Audio formats (`.adp4`, `.adps`) and a WAV export.
3. Script bytecode: opcode table and a disassembler.
4. Runtime: window, rendering, text, audio and input on SDL.
5. Video playback, save/load, settings.

## Legal

This is a clean reimplementation written from observed file formats and
behaviour. It contains no code or assets from the original games. Pure Mail
and Overflow are trademarks of their respective owners; this project is not
affiliated with them.
