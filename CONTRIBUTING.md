# Contributing to VNRevival

Thank you for helping. Bug reports, game requests, testing on systems
the author does not have (macOS and Linux especially) and pull requests
are all welcome.

## Never share game data

VNRevival only works with copies people own, so no copyrighted game data
goes into this repository, its issues or its pull requests. That means
no game files, extracted pictures or sounds, scripts or original
executables. Screenshots to show a bug are fine. The `.gitignore` blocks
the usual data files (`*.DAT`, `*.AVI`, `*.SAV`), but check what you add.

## Reporting bugs

Use the [bug report form](https://github.com/Yamato6/VNRevival/issues/new/choose)
and include:

* your system and the VNRevival version (shown in the main menu),
* the game and its edition,
* what you did, what you expected and what happened,
* what VNRevival printed, if you can start it from a terminal
  (`vnrevival run "<game folder>"`). Engine functions that are not
  implemented yet are listed there.

## Code

Read [docs/DEVELOPING.md](docs/DEVELOPING.md) for the architecture and
how to build. Then:

* Keep `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`
  and `cargo test` clean; CI runs them on Windows, Linux and macOS.
* `unsafe` code is not allowed in this workspace.
* Code, comments, docs and commit messages are in English. Commit
  messages say what the change does for the user, in plain words
  ("Play the endings' staff roll"), not in prefixes.
* Behaviour taken from an original engine should match it. Say in the
  code where it comes from (the function or address in the original
  executable) when that is not obvious.
* Reading a new file format belongs in
  [VNTools-rs](https://github.com/Yamato6/VNTools-rs); turning it into
  something playable belongs here. See
  [Working on VNTools-rs at the same time](docs/DEVELOPING.md#working-on-vntools-rs-at-the-same-time).
* Interface text goes through `crates/vnr-core/src/lang.rs`, with
  English, Spanish and Japanese.

For larger changes, such as a new engine, please open an issue first so
we can agree on the approach.

By contributing, you agree that your work is released under the
[MIT license](LICENSE).
