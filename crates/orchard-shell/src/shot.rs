//! `--screenshot OUT.png|OUT.webp`: the parts every app's headless shot
//! shares (docs, and checking the look without a window): the common
//! options, the renderer, and saving the image. Building the state to show
//! (demo data, dialogs) stays with each app.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use orchard_grid::{FontConfig, Frame, HeadlessRenderer, RgbaImage};

/// The renderer's scale factor for screenshots.
pub const SCALE: f32 = 2.0;

/// The options every screenshot has.
#[derive(Clone, Debug, PartialEq)]
pub struct ShotArgs {
    pub out: PathBuf,
    /// Columns and rows.
    pub size: (u16, u16),
    /// `--theme NAME`: a color scheme.
    pub theme: Option<String>,
    /// `--dialog NAME`: what the app opens over its main view.
    pub dialog: Option<String>,
}

/// `args` after `--screenshot`: the output file, then `--size COLSxROWS`
/// (either half keeps `default_size`'s if it doesn't parse), `--theme` and
/// `--dialog`. Every other argument goes to `extra`, with the iterator to
/// take its value from (unknown ones are ignored). Without an output file:
/// says so, and `Err` is the exit code.
pub fn parse(
    args: &[String],
    default_size: (u16, u16),
    mut extra: impl FnMut(&str, &mut std::slice::Iter<'_, String>),
) -> Result<ShotArgs, ExitCode> {
    let Some(out) = args.first().map(PathBuf::from) else {
        eprintln!("--screenshot needs an output file");
        return Err(ExitCode::FAILURE);
    };
    let mut o = ShotArgs { out, size: default_size, theme: None, dialog: None };
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--size" => {
                if let Some((c, r)) = it.next().and_then(|s| s.split_once('x')) {
                    o.size = (c.parse().unwrap_or(o.size.0), r.parse().unwrap_or(o.size.1));
                }
            }
            "--theme" => o.theme = it.next().cloned(),
            "--dialog" => o.dialog = it.next().cloned(),
            other => extra(other, &mut it),
        }
    }
    Ok(o)
}

/// The headless renderer for `font` at [`SCALE`]; says why if there is none.
pub fn renderer(font: FontConfig) -> Result<HeadlessRenderer, ExitCode> {
    HeadlessRenderer::new(font, SCALE).map_err(|e| {
        eprintln!("renderer: {e}");
        ExitCode::FAILURE
    })
}

/// A cell's size in pixels (for the app's `resized`).
pub fn cell_px(r: &mut HeadlessRenderer) -> (f32, f32) {
    let m = r.renderer().metrics();
    (m.width as f32, m.height as f32)
}

/// Render `frame` and save it to `out` ([`save_image`]); says why not.
pub fn finish(r: &mut HeadlessRenderer, frame: &Frame, out: &Path) -> ExitCode {
    let saved = r.render(frame).map_err(|e| e.to_string()).and_then(|img| save_image(&img, out));
    match saved {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("screenshot: {e}");
            ExitCode::FAILURE
        }
    }
}

/// PNG, or lossless WebP for a `.webp` name.
pub fn save_image(img: &RgbaImage, out: &Path) -> Result<(), String> {
    let webp = out.extension().is_some_and(|e| e.eq_ignore_ascii_case("webp"));
    if !webp {
        return img.save_png(out).map_err(|e| e.to_string());
    }
    let buf = image::RgbaImage::from_raw(img.width, img.height, img.data.clone()).ok_or("bad image size")?;
    buf.save_with_format(out, image::ImageFormat::WebP).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn common_options_and_extras() {
        let mut extras = Vec::new();
        let o = parse(
            &args(&["out.png", "--size", "90x30", "--music", "/m", "--theme", "nord", "--shuffle", "--dialog", "help"]),
            (112, 38),
            |a, it| {
                let value = if a == "--music" { it.next().cloned() } else { None };
                extras.push((a.to_string(), value));
            },
        )
        .unwrap();
        assert_eq!(
            o,
            ShotArgs { out: "out.png".into(), size: (90, 30), theme: Some("nord".into()), dialog: Some("help".into()) }
        );
        assert_eq!(extras, [("--music".to_string(), Some("/m".to_string())), ("--shuffle".to_string(), None)]);
    }

    #[test]
    fn sizes_fall_back_per_half() {
        let size = |s: &str| parse(&args(&["o.png", "--size", s]), (120, 40), |_, _| {}).unwrap().size;
        assert_eq!(size("90xbad"), (90, 40));
        assert_eq!(size("x30"), (120, 30));
        assert_eq!(size("90"), (120, 40), "no x: the value is taken, the size kept");
        let o = parse(&args(&["o.png", "--size", "90", "--theme", "nord"]), (120, 40), |_, _| {}).unwrap();
        assert_eq!(o.theme.as_deref(), Some("nord"));
    }

    #[test]
    fn an_output_file_is_needed() {
        assert_eq!(parse(&[], (80, 24), |_, _| panic!()), Err(ExitCode::FAILURE));
    }

    #[test]
    fn saves_png_and_lossless_webp() {
        let dir = tempfile::tempdir().unwrap();
        let data: Vec<u8> = (0..4 * 3 * 2).map(|i| (i * 37 % 256) as u8 | 0x0f).collect();
        let mut data = data;
        for px in data.chunks_mut(4) {
            px[3] = 255;
        }
        let img = RgbaImage { width: 3, height: 2, data: data.clone() };
        for name in ["shot.png", "shot.WebP"] {
            let path = dir.path().join(name);
            save_image(&img, &path).unwrap();
            let back = image::open(&path).unwrap().to_rgba8();
            assert_eq!((back.width(), back.height()), (3, 2), "{name}");
            assert_eq!(back.into_raw(), data, "{name}: lossless");
        }
        let png = std::fs::read(dir.path().join("shot.png")).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let webp = std::fs::read(dir.path().join("shot.WebP")).unwrap();
        assert_eq!(&webp[8..12], b"WEBP");
    }
}
