# Orchard Commons

Code shared by the Orchard apps: Orchard Commander (the two-panel file
manager), Orchard Music Player and Orchard Download Manager. All of them are
GPU-drawn desktop apps styled like terminal programs, and they share the
renderer, the color schemes and the look.

| Crate | What it is |
|---|---|
| `orchard-grid` | GPU character-grid renderer (wgpu, winit): a `Frame` of cells, boxes, images and a cursor, drawn into a window or an offscreen image |
| `orchard-theme` | Color schemes (built-ins plus the user's, in Orchard Commander's `color-schemes/` folder, so a scheme made in its editor shows up in every app), grid `Colors`, the `Theme` (scheme, frame style, font) and where Orchard Commander keeps its config |
| `orchard-kit` | The app UI kit: background tasks, key and mouse events (converted from winit's), the box frame, scrollbar, F-key bar and text field, clipboard paste (default `clipboard` feature) |
| `orchard-metadata` | File metadata (for rename placeholders and tag display): audio tags, video and image properties, photo EXIF, and a registry for more providers; reads any seekable stream, independent of where files live |

Lower crates stay free of any UI framework; only `orchard-grid` (and the
app kit built on it) know about windows.

## Using it from an app

Depend on a released tag:

```toml
[dependencies]
orchard-grid = { git = "https://github.com/sandboxly/orchard-commons", tag = "v0.1.0" }
orchard-theme = { git = "https://github.com/sandboxly/orchard-commons", tag = "v0.1.0" }
```

In a workspace, put these under `[workspace.dependencies]` and use
`orchard-grid.workspace = true` in the member crates.

## Developing against a local checkout

To try changes here in an app before tagging, point the git dependency at
this checkout with a `[patch]`, in the app repository's `.cargo/config.toml`
(keep it out of version control, e.g. in `.git/info/exclude`):

```toml
[patch."https://github.com/sandboxly/orchard-commons"]
orchard-grid = { path = "/path/to/orchard-commons/crates/orchard-grid" }
orchard-theme = { path = "/path/to/orchard-commons/crates/orchard-theme" }
orchard-kit = { path = "/path/to/orchard-commons/crates/orchard-kit" }
orchard-metadata = { path = "/path/to/orchard-commons/crates/orchard-metadata" }
```

One line per crate the app uses. Remove the file to go back to the tag.

## Building and testing

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p orchard-grid --release --example demo
```

The renderer tests draw offscreen and skip (with a note) without a GPU
adapter or fonts. Without a GPU (CI, VMs) the renderer falls back to a
software adapter.

## License

MIT, see [LICENSE](LICENSE).
