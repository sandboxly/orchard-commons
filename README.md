# Orchard Commons

Code shared by the Orchard apps: Orchard Commander (the two-panel file
manager), Orchard Music Player and Orchard Download Manager. All of them are
GPU-drawn desktop apps styled like terminal programs, and they share the
renderer, the color schemes and the look.

| Crate | What it is |
|---|---|
| `orchard-grid` | GPU character-grid renderer (wgpu, winit): a `Frame` of cells, boxes, images and a cursor, drawn into a window or an offscreen image |
| `orchard-theme` | Color schemes (built-ins plus the user's, in Orchard Commander's `color-schemes/` folder, so a scheme made in its editor shows up in every app), grid `Colors`, the `Theme` (scheme, frame style, font) and where Orchard Commander keeps its config |
| `orchard-kit` | The app UI kit: background tasks, key and mouse events (key events converted from winit's; media keys are left to the app), the box frame, scrollbar, F-key bar and text field (paste and ⌘⌫ unless `plain`), dialog forms (combo box, spin box, checkbox, buttons, a laid-out form with one focused control), a hotkey menu with submenus (the bookmark menu), the file and folder picker (places column, Orchard Commander's bookmarks on ⌘D, marking, type-ahead, listing off the UI thread), clipboard paste (default `clipboard` feature) |
| `orchard-places` | What the pickers offer besides the listing: Orchard Commander's bookmarks (its `bookmarks.json`, read fresh and added to safely while other apps use it, so a folder bookmarked in one app shows up in all of them), the home folder, the standard folders, drives and mounted volumes |
| `orchard-shell` | The window, event loop and command line of the Music Player and Download Manager: `run` with the app behind the `ShellApp` trait, `--help`/`--version`/`--self-test`/`--screenshot`, the crash log, headless screenshots (PNG, WebP), the app's config folder and JSON settings files |
| `orchard-metadata` | File metadata (for rename placeholders and tag display): audio tags, video and image properties, photo EXIF, and a registry for more providers; reads any seekable stream, independent of where files live |

Lower crates (`orchard-places`, `orchard-metadata`) stay
free of any UI framework; only `orchard-grid`, the app kit
and `orchard-shell` know about windows, and only `orchard-shell` runs an event
loop.

## Using it from an app

Depend on a released tag:

```toml
[dependencies]
orchard-grid = { git = "https://github.com/sandboxly/orchard-commons", tag = "v0.3.0" }
orchard-theme = { git = "https://github.com/sandboxly/orchard-commons", tag = "v0.3.0" }
```

In a workspace, put these under `[workspace.dependencies]` and use
`orchard-grid.workspace = true` in the member crates.

## Developing against a local checkout

To try changes here in an app before tagging, point the git dependency at
this checkout with a `[patch]`:

```toml
[patch."https://github.com/sandboxly/orchard-commons"]
orchard-grid = { path = "/path/to/orchard-commons/crates/orchard-grid" }
orchard-theme = { path = "/path/to/orchard-commons/crates/orchard-theme" }
orchard-kit = { path = "/path/to/orchard-commons/crates/orchard-kit" }
orchard-metadata = { path = "/path/to/orchard-commons/crates/orchard-metadata" }
orchard-shell = { path = "/path/to/orchard-commons/crates/orchard-shell" }
orchard-places = { path = "/path/to/orchard-commons/crates/orchard-places" }
```

One line per crate the app uses. Keep it out of version control:

- The apps already track a `.cargo/config.toml` (Windows linker flags), so
  either add the patch there and hide the edit with
  `git update-index --skip-worktree .cargo/config.toml` (undo with
  `--no-skip-worktree` and `git checkout .cargo/config.toml`), or put it in an
  untracked file listed in `.git/info/exclude` and pass it after the
  subcommand: `cargo build --config .cargo/commons-patch.toml`.
- Cargo still fetches the git dependency's repository and tag before applying
  a patch, so the tag the app names must exist upstream (or in Cargo's git
  cache). The patch may also bring a crate that tag doesn't have yet (a
  new crate here): name it in `[dependencies]` with the same git URL and tag
  and it resolves through the patch.
- A patched build records the crates as local paths in `Cargo.lock`. Don't
  commit that: build without `--locked` while patched, and before committing
  the lockfile run `cargo update -p orchard-grid -p orchard-theme …` without
  the patch so it points at the git tag again (CI builds `--locked`).

## Building and testing

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p orchard-grid --release --example demo
cargo run -p orchard-shell --example hello       # the shell, kit and a form
cargo run -p orchard-shell --example picker      # the file picker (add `files` for files)
```

Tests never touch the real config folder: the picker's tests pass their own
bookmarks file, and the places tests only read.

The renderer tests draw offscreen and skip (with a note) without a GPU
adapter or fonts. Without a GPU (CI, VMs) the renderer falls back to a
software adapter.

## License

MIT, see [LICENSE](LICENSE).
