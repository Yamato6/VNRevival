# VNRevival user manual

VNRevival plays old visual novels from the files of **your own copy** of
the game, on today's Windows, Linux and macOS. It does not come with any
game: you need the game installed (or copied) on your computer first.

Supported today: **Pure Mail** (Overflow, 2001), the DVD edition.

* [Starting VNRevival](#starting-vnrevival)
* [The library](#the-library)
* [The main menu](#the-main-menu)
* [Playing](#playing)
* [The hero's name](#the-heros-name)
* [Saving and loading](#saving-and-loading)
* [The history](#the-history)
* [Extras](#extras)
* [Settings](#settings)
* [Fonts](#fonts)
* [Where VNRevival keeps your files](#where-vnrevival-keeps-your-files)
* [Problems](#problems)

## Starting VNRevival

Download the package for your system from the project's releases and
open it:

* **Windows** (`…-windows-x64.zip`): extract the whole folder anywhere
  and open `VNRevival.exe`. Keep the `fonts` folder next to it. If
  Windows SmartScreen warns about an unknown program, click *More info*
  and *Run anyway*.
* **macOS** (`…-macos.dmg`, for Intel and Apple Silicon Macs, macOS 11
  or newer): open the disk image and drag **VNRevival** to
  *Applications*. The app is not notarised by Apple, so the first time
  macOS refuses to open it: open it, then go to *System Settings →
  Privacy & Security* and click *Open Anyway* (on older versions,
  right-click the app and choose *Open*).
* **Linux** (`…-linux-x64.AppImage`): make it executable
  (`chmod +x VNRevival-*.AppImage`, or the file's properties) and open
  it. The `.tar.gz` holds the same program as a plain folder: extract it
  and run `vnrevival`, keeping the `fonts` folder next to it.

VNRevival opens on the [library](#the-library). From a terminal,
`vnrevival run "<game folder>"` goes straight to one game (it is added
to the library too), and `vnrevival --help` lists the other options. If you build VNRevival
yourself, `cargo run --release` does the same; the
[README](../README.md) explains how to build it.

The window can be resized and the picture keeps its shape. **Alt+Enter**
switches between a window and full screen.

## The library

The library shows the games you have added, the last played first, each
with a picture from the game and when you last played it.

* **Add game…** opens a folder browser. Go to the folder where the game
  is installed: for Pure Mail, the one with `PMAIL.EXE`, `OF_ADV2.INI`
  and the `PACK` folder. When VNRevival recognises it, the game's name
  appears at the bottom; click **Use this folder**. The drives are listed
  on the left (Windows); `..` goes up one folder.
* Click a game to play it.
* The **×** on a game takes it out of the library. Its files and your
  saves are not touched.
* **Settings** opens the [settings](#settings); **Quit** closes
  VNRevival.

## The main menu

Every game opens on VNRevival's main menu:

* **New game** starts from the beginning.
* **Load** continues a saved game.
* **Extras**: the CG gallery, the music room and scene replay.
* **Settings**.
* **Change game** goes back to the library.
* **Quit**.

If you prefer the game's own title screen, set *System → At start* to
the game's title. Its buttons then work as in the original: its load
button opens VNRevival's load screen, and its extras open VNRevival's.

## Playing

| Action | Mouse | Keyboard |
|---|---|---|
| Next message | left click | Enter or Space |
| Skip (hold) | | Ctrl |
| Auto mode on/off | pause menu | A |
| Hide the message window | middle button | |
| Pause menu | right click | Escape |
| History | wheel up | Page Up |
| Save / load / settings | pause menu | F2 / F3 / F4 |

**Skipping** stops at text you have never read, in any playthrough,
unless *Reading → Skip unread text* is on.

**Auto mode** moves on by itself once a message is shown; "AUTO" appears
in the corner while it is on. How long it waits depends on the length of
the message and on *Reading → Auto mode speed*; it can also wait for the
voice to end. A choice always waits for you.

The **pause menu** has: back to the game, save, load, history, auto mode,
settings, main menu and quit. Escape or a right click closes any screen.

## The hero's name

When a new game of Pure Mail starts, VNRevival asks for the hero's name,
as the original did:

* surname and given name, as they are written (緒方 圭 by default);
* their readings (おがた, ケイ);
* the handle he uses in the chat (Ａ，Ｗ).

Click a field, or move between fields with the arrow keys, and type. For
Japanese, use your system's input method (IME) as in any other program:
the text being composed is shown underlined and its candidate window
opens next to the field. Backspace deletes, Enter goes to the next
field, and **Start** (or Enter after the last field, or Escape) begins
the game. **Restore defaults** brings back the original names.

The voices say the hero's name only while both readings and the handle
are the original ones; with other names those lines are voiced without
it, as in the original game.

You can change the names later in *Settings → Game*. The change applies
from the next new game; a saved game keeps the names it was played with.

## Saving and loading

Each game has **10 slots**. You can save whenever the game is waiting
for you: a message, a choice, a chat line or an open mail. Each slot
shows when it was saved, the chapter and the text on screen. Saving over
a slot replaces it.

Saves made with the original program cannot be loaded, and VNRevival's
saves cannot be used by the original.

## The history

The history lists the messages and chat lines of the current session,
with who said them. Scroll with the wheel, the arrow keys or Page Up and
Page Down; scrolling down past the end closes it. Lines with a voice
have a small mark: click them to hear the voice again.

## Extras

* **Gallery**: the pictures you have seen. Click one to see it full
  screen; when a picture has variants, click or use the arrow keys or
  the wheel to see the next one.
* **Music**: the tracks you have heard. Click one to play it.
* **Scenes**: replay a scene you have reached. At its end you come back
  to the list.

What you have not seen yet shows as "？". *System → Unlock all extras*
opens everything. For Pure Mail, progress made with the original program
counts too: VNRevival reads its records (`SAVE\SAVEFL99.SAV`) the first
time.

## Settings

Settings (F4) apply to every game. Click a value, or select a row with
the arrow keys and change it with Left and Right; Page Up and Page Down
change tabs.

**System**

* *Language*: English, Spanish or Japanese, for VNRevival's own screens
  (the game's text is not translated).
* *Interface font*: the font of VNRevival's screens.
* *At start*: VNRevival's main menu or the game's own title screen.
* *Full screen*.
* *Unlock all extras*.

**Sound**: master, music, sound effects and voices.

**Text** (with a preview of the result)

* *Game text font*: the font of the game's messages, choices, the PC
  screens and the history.
* *Text size*: from 18 to 30 pixels (22 is the original size). The
  message window grows when larger text needs more room.
* *Line spacing*.
* *Text edge*: none, a shadow (as the original) or a full outline.
* *High contrast*: white text on an opaque black window.
* *Window opacity*.

**Reading**

* *Text speed*: up to instant.
* *Auto mode speed*.
* *Auto mode waits for voices*.
* *Skip unread text*.

**Game** appears for games with options of their own: for Pure Mail,
[the hero's name](#the-heros-name).

## Fonts

By default the screens and the game text use **Noto Sans JP**, a modern
font that is easy to read. You can choose among the Japanese fonts that
are installed: Noto Sans JP, Noto Serif JP, BIZ UDPGothic, BIZ
UDPMincho, Yu Gothic and Meiryo. **Original** uses the fonts the game was
made for (ＭＳ Ｐゴシック and ＭＳ Ｐ明朝) or the closest ones installed.

Noto Sans JP comes with VNRevival (in the `fonts` folder of the
download), so it is always there. The others can be installed for free:
Noto Serif JP from Google Fonts, or the `fonts-noto-cjk` package on
Linux; BIZ UDP fonts, Yu Gothic and Meiryo come with Windows. If the
chosen font is missing, the next available one is used.

## Where VNRevival keeps your files

| System | Folder |
|---|---|
| Windows | `%APPDATA%\VNRevival` |
| macOS | `~/Library/Application Support/VNRevival` |
| Linux | `~/.local/share/vnrevival` |

In that folder:

* `settings.json`: your settings.
* `library.json` and `covers/`: the library.
* `saves/<game>/`: the saves of each game, plus what belongs to that game:
  the names (`options.json`), what you have read (`read.json`), the music
  heard (`music.json`) and the records the extras use (`system.sav`).
* `lang/<code>.json` (optional): your own wording for an interface
  language (`en`, `es` or `ja`), as `{"key": "text"}` pairs.

The game's own folder is only read, never changed.

## Problems

**VNRevival shows an error and closes.** The message says what went
wrong (for example a game folder that cannot be read). To see the
warnings VNRevival writes while it runs, start it from a terminal.

**The folder browser does not recognise the game.** Make sure you are in
the folder that holds the game's program and data, not one above or
below it. For Pure Mail it must have `PMAIL.EXE`, `OF_ADV2.INI` and
`PACK\SCRIPT.DAT`.

**There is no sound.** VNRevival still runs without a sound device. Check
the volumes in *Settings → Sound* and that the system has an output
device.

**The text looks different from the original.** Set *Game text font* to
*Original* and the text edge to *Shadow*.

**Typing Japanese does not work.** Switch your input method to Japanese
before clicking the field, or while it is selected. The input method is
only active while a field is selected, so it does not take the game's
keys.
