# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Orchard Commons: the code the Orchard apps share. Orchard Commander (`../orchard-commander`, a classic two-panel file manager), Orchard Music Player and Orchard Download Manager are GPU-drawn desktop apps (winit + wgpu) styled like terminal programs; they depend on these crates by git tag (`https://github.com/sandboxly/orchard-commons`, `tag = "vX.Y.Z"`) and develop against a local checkout through an uncommitted `[patch]` (see README: the apps track `.cargo/config.toml`, the tag must already exist upstream, and a patched `Cargo.lock` must not be committed). Code moved here from the apps keeps its comments and tests.

## Commands

```sh
cargo test --workspace                                 # all tests
cargo test -p orchard-theme schemes                    # a subset
cargo clippy --workspace --all-targets -- -D warnings  # lint (CI uses -D warnings)
cargo run -p orchard-grid --release --example demo     # the renderer in a window
cargo run -p orchard-shell --example hello             # the shell: a form in a window (also --self-test, --screenshot OUT.png)
```

- `ORCHARD_COMMANDER_CONFIG_DIR=<empty dir>` points `orchard_theme::dirs` (and Orchard Commander) at another config directory.
- Renderer tests draw offscreen and skip with a note without a GPU adapter or fonts; without a GPU `orchard-grid` uses a software adapter (CI installs `mesa-vulkan-drivers`).

## Crates

- `orchard-grid`: the wgpu character-grid renderer (`Frame`, `Style`, `BoxStyle`, `FontConfig`, `HeadlessRenderer`, `WindowRenderer`); re-exports `wgpu` and `winit` so apps match its versions.
- `orchard-theme` (on `orchard-grid`): `schemes` (`ColorScheme`, built-ins, scheme TOML files, `Appearance`, frame styles and font constants), `colors` (`Colors`, `mix`), `theme` (`Theme`), `dirs` (Orchard Commander's config directory and its `color-schemes/` folder, shared by every app).
- `orchard-kit` (on `orchard-grid` + `orchard-theme`): the app UI kit the Music Player and Download Manager share: `tasks` (worker pool, results run on the UI thread), `input` (key/mouse events), `winit` (winit key events converted; media keys return `None`, the Music Player maps play/pause itself), `widgets` (`DrawCtx`, `BoxFrame`, `Scrollbar`, F-key bar, `TextField`: inset one cell unless `flush`; ⌘V / Ctrl+V paste and ⌘⌫ / Ctrl+Backspace delete-to-start unless `plain`, which ignores modifiers), `form` (dialog boxes and form controls: combo box and its `Popup`, spin box, checkbox, buttons, headings; `Form<I>` lays out controls with one focused and turns keys and clicks into `Act`s; values always come from the caller), `clipboard` (`text()`, behind the default `clipboard` feature). Orchard Commander keeps its own versions in `oc-ui`.
- `orchard-shell` (on `orchard-grid` + `orchard-kit`): the Music Player's and Download Manager's `main` around their `App`: `run(&AppInfo, make)` (winit event loop, window and GPU surface, redraws and animation frames, font changes, mouse cells and double clicks, `<APP>_EXIT_AFTER_START=1`), the app behind `ShellApp` (required methods every app has, hooks with fixed places in the loop for what only one app does), `cli` (`common_args`, `self_test` with `Check`s, `render_check`, `log_panics` to `crash.log`, Windows `attach_parent_console`), `shot` (`--screenshot`'s common options, headless renderer at scale 2, PNG / lossless WebP), `config` (`app_config_dir` with its env override, `write_atomic` via "<name>.tmp", `load_json` / `save_json`). Each app keeps its usage text, screenshot state and dialogs, `Settings` struct and `windows_subsystem` attribute. Orchard Commander doesn't use it.
- `orchard-metadata` (no UI, no file-system layer): `MetadataProvider`s (`AudioTags`, `MediaProperties`, `PhotoExif`) and the `Registry` (global `registry()` / `register()`), read with `Registry::read_source` from any seekable stream; value helpers (`clean`, `duration`, `format_g`). Orchard Commander's `oc-rename` wraps it for its virtual file systems (opening, streaming, cloud-only files).

## Rules

- **Compatibility**: color-scheme files, the `color-schemes/` folder location and the config directory must stay exactly what Orchard Commander (and its Python 1.x) read and write. Behaviour changes here change every app.
- **Lower crates stay UI-framework-free**: only `orchard-grid`, the app kit and `orchard-shell` may know about windows, events or drawing; window-system code (the event loop, window creation) stays in `orchard-shell`, out of the kit.
- **The shell moves behaviour, it doesn't choose it**: output text (`--version`, self-test lines, error prefixes, "opened, drew a frame and closed"), settings file names and format, env vars and CLI flags must stay what the apps had. Something only one app does is a `ShellApp` hook at the exact point of the loop where that app did it.
- **UI copy rules live in each app** (Orchard Commander: `docs/ai-microcopy-guide.md`); user-visible strings here follow them too.
- **No third-party product names** in user-visible text, docs or code comments (describe the behaviour instead), except where a license requires naming a component.
- **Tests must never touch the real config dir**: use `tempfile` dirs and pass paths explicitly.
- After a change an app needs, tag a new version here and bump the tag in the apps.
