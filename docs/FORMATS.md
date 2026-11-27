# OverflowADV2 data formats

Reverse-engineering notes for the OverflowADV2 engine, gathered from the
Pure Mail DVD release (`PMAIL.EXE`, 2006 build; data files from 2001).
Everything here was verified against the shipped files unless marked
**unverified**.

Status legend: ✅ understood and implemented · 🟡 partly understood · ❓ unknown

## Install layout

```
PMAIL.EXE          engine (MSVC, Win32, DirectDraw era)
PMCFG.EXE          settings tool
OF_ADV2.INI        settings, section [OverflowADV2]
LOCPU.DLL          graphics/LZ helpers, plain x86
MMX.DLL            same exports, MMX build
SSE.DLL            same exports, SSE build
SSE2.DLL           same exports, SSE2 build (chosen by CpuModule= in the INI)
PACK\GRAPHIC.DAT   images
PACK\SCRIPT.DAT    compiled scripts
PACK\SOUND.DAT     music and sound effects
PACK\VOICE.DAT     voices
MOVIE\*.AVI        opening movies, Indeo 5 video (FourCC IV50)
SAVE\*.SAV         save data
```

The executable also references `PACK\SUB.DAT`, which the DVD release does
not ship.

## CPU helper DLLs ✅

All four DLLs export the same functions; `PMAIL.EXE` loads the one named by
`CpuModule=`:

`AddRgbChr AddSaturation AlpBlend AlpBlend4 AlpBlend4S AlpBlend4SS BufCopy
ColOffset Cpu DecodeChr DecodeLz DecodeMapChr Effect Fill FillRGB GetCpuid
GetDllInfo HalfTone25P HalfTone50P Init LoadChr LoadMapChr LoadRgb RectCopy
RectGet RectInGet RectInSet RectSet SkipLineChr SubRgbChr SubSaturation XFade`

They are the reference for image decoding (`DecodeChr`, `DecodeMapChr`,
`LoadRgb`, …) and for blending and transition effects. `LOCPU.DLL` is the
easiest to read because it has no SIMD code.

## LZSS (`DecodeLz`) ✅

`DecodeLz(dst, dst_len, src, src_len)`, disassembled from `LOCPU.DLL`
(`0x10001470`):

* 4096-byte ring buffer filled with zeros; the first write goes to `0xFEE`.
* A flag byte covers the next 8 items and is read **MSB first**.
  * bit = 1 → back-reference: little-endian `u16 w`;
    position = `w >> 4`, length = `(w & 0xF) + 3`.
  * bit = 0 → one literal byte.
* Every output byte is also written into the ring buffer. Copies go byte by
  byte, so overlapping references repeat data.
* Decoding stops after `dst_len` bytes.

Implementation: `crates/adv2-formats/src/lzss.rs`.

## Pack archives (`*.DAT`) ✅

```
offset 0                                          file end
| member data, back to back | index (LZSS) | trailer (12) |
```

### Trailer

The last 12 bytes, **every byte XORed with `0xF0`**, hold three little-endian
`u32` values:

| # | Meaning |
|---|---|
| 0 | packed (LZSS) index size |
| 1 | unpacked index size (multiple of 80) |
| 2 | ❓ unknown, different in every pack (checksum?) |

The index is at `file_size - 12 - packed_size`. It is LZSS-compressed but
**not** XORed.

### Index entry (80 bytes)

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | header bytes (see below) |
| 4 | 64 | file name, Shift-JIS, NUL-padded |
| 68 | 4 | data offset from the start of the file |
| 72 | 4 | stored (packed) size |
| 76 | 4 | unpacked size |

Header bytes:

| Byte | Meaning |
|---|---|
| 0 | ❓ varies per script (`.obj`), 0 elsewhere |
| 1 | ❓ 0, 1 or 2 in scripts, 0 elsewhere |
| 2 | ✅ `1` = member is LZSS-compressed, `0` = stored |
| 3 | ❓ 1 in scripts, 0 elsewhere |

Members are stored back to back: entry *n+1* starts at offset *n* + packed
size *n*, and the last one ends exactly where the index starts.

### Pure Mail DVD contents

| Pack | Entries | Members |
|---|---|---|
| SCRIPT.DAT | 385 | `.obj` (all compressed) |
| GRAPHIC.DAT | 2359 | `.crgb` 632, `.anim` 620, `.char` 395, `.edg` 234, `.cgg` 165, `.cgm` 165, `.img` 73, `.alpf` 50, `.rol` 15, `.map` 4, `.alpm` 3, `.alpa` 1, `.msk` 1, `.dib` 1 |
| SOUND.DAT | 544 | `.adp4` 515, `.adps` 29 (all stored) |
| VOICE.DAT | 15429 | `.adp4` (all stored) |

Implementation: `crates/adv2-formats/src/pack.rs`.

## Graphics 🟡

* `.dib` ✅: raw 24-bit BGR pixels, bottom-up rows, no header (like a BMP
  without its headers). `hiro17.DIB` is 2 328 000 bytes = 800 × 970 × 3;
  the width of 800 is confirmed visually, the height is derived.
* `.crgb`, `.char`, `.anim`, `.edg`, `.cgg`, `.cgm`, `.img`, `.alp*`,
  `.map`, `.msk`, `.rol` ❓: not analysed yet. `DecodeChr`, `LoadChr`,
  `LoadRgb` and `LoadMapChr` in the CPU DLLs are the starting point.
  `.alpf` members are exactly 30 000 bytes and `.cgg`/`.cgm` exactly
  57 600 bytes each, which suggests fixed-size masks or tables.

## Scripts (`.obj`) ❓

Compiled bytecode. Decompressed `start.obj` begins with a table of
`(u16, u16)` pairs, followed by code and Shift-JIS strings such as
`edhi01`…`edhi10` (ending names). Not analysed yet; the interpreter lives
in `PMAIL.EXE`.

## Audio (`.adp4`, `.adps`) ❓

Stored uncompressed in the packs. The extensions suggest 4-bit ADPCM
(`adps` is probably the stereo variant). Not analysed yet.

## Video ✅ (identified)

`MOVIE\PM01L_I5.AVI` and `PM01S_I5.AVI` are AVI files with Indeo 5 video
(FourCC `IV50`). The executable also knows `_I3` variants (Indeo 3). FFmpeg
decodes both.
