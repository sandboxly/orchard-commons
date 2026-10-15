# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Orchard Commons: the code the Orchard apps share. Orchard Commander (`../orchard-commander`, a classic two-panel file manager), Orchard Music Player and Orchard Download Manager are GPU-drawn desktop apps (winit + wgpu) styled like terminal programs; they depend on these crates by git tag (`https://github.com/sandboxly/orchard-commons`, `tag = "vX.Y.Z"`) and develop against a local checkout through an uncommitted `[patch]` in their `.cargo/config.toml` (see README). Code moved here from the apps keeps its comments and tests.

## Commands

```sh
cargo test --workspace                                 # all tests
cargo test -p orchard-theme schemes                    # a subset
cargo clippy --workspace --all-targets -- -D warnings  # lint (CI uses -D warnings)
cargo run -p orchard-grid --release --example demo     # the renderer in a window
```

- `ORCHARD_COMMANDER_CONFIG_DIR=<empty dir>` points `orchard_theme::dirs` (and Orchard Commander) at another config directory.
- Renderer tests draw offscreen and skip with a note without a GPU adapter or fonts; without a GPU `orchard-grid` uses a software adapter (CI installs `mesa-vulkan-drivers`).

## Crates

- `orchard-grid`: the wgpu character-grid renderer (`Frame`, `Style`, `BoxStyle`, `FontConfig`, `HeadlessRenderer`, `WindowRenderer`); re-exports `wgpu` and `winit` so apps match its versions.
- `orchard-theme` (on `orchard-grid`): `schemes` (`ColorScheme`, built-ins, scheme TOML files, `Appearance`, frame styles and font constants), `colors` (`Colors`, `mix`), `theme` (`Theme`), `dirs` (Orchard Commander's config directory and its `color-schemes/` folder, shared by every app).
- `orchard-kit` (on `orchard-grid` + `orchard-theme`): the app UI kit the Music Player and Download Manager share: `tasks` (worker pool, results run on the UI thread), `input` (key/mouse events), `winit` (winit key events converted; the play/pause media key becomes Space), `widgets` (`DrawCtx`, `BoxFrame`, `Scrollbar`, F-key bar, `TextField`, inset one cell unless `flush`), `clipboard` (`text()`, behind the default `clipboard` feature). Orchard Commander keeps its own versions in `oc-ui`.
- `orchard-metadata` (no UI, no file-system layer): `MetadataProvider`s (`AudioTags`, `MediaProperties`, `PhotoExif`) and the `Registry` (global `registry()` / `register()`), read with `Registry::read_source` from any seekable stream; value helpers (`clean`, `duration`, `format_g`). Orchard Commander's `oc-rename` wraps it for its virtual file systems (opening, streaming, cloud-only files).

## Rules

- **Compatibility**: color-scheme files, the `color-schemes/` folder location and the config directory must stay exactly what Orchard Commander (and its Python 1.x) read and write. Behaviour changes here change every app.
- **Lower crates stay UI-framework-free**: only `orchard-grid` and the app kit may know about windows, events or drawing.
- **UI copy rules live in each app** (Orchard Commander: `docs/ai-microcopy-guide.md`); user-visible strings here follow them too.
- **No third-party product names** in user-visible text, docs or code comments (describe the behaviour instead), except where a license requires naming a component.
- **Tests must never touch the real config dir**: use `tempfile` dirs and pass paths explicitly.
- After a change an app needs, tag a new version here and bump the tag in the apps.
