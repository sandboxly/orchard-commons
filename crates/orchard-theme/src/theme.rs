//! The look: a color scheme resolved to grid colors, plus the frame style
//! and font.

use orchard_grid::{BoxStyle, FontConfig, Style};

use crate::Colors;
use crate::schemes::{self, Appearance, ColorScheme};

/// Everything drawing needs to know about the look.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub scheme: ColorScheme,
    pub colors: Colors,
    pub frame: BoxStyle,
    pub font: FontConfig,
}

impl Theme {
    pub fn new(appearance: &Appearance, scheme: ColorScheme) -> Theme {
        let font = FontConfig {
            family: appearance.font_family.clone(),
            style: appearance.font_style.clone(),
            size: appearance.font_size as f32,
            line_spacing: appearance.line_spacing as f32,
            fallback: Vec::new(),
        }
        .sanitized();
        Theme {
            colors: Colors::from_scheme(&scheme),
            scheme,
            frame: BoxStyle::from_name(&appearance.frame_style).unwrap_or_default(),
            font,
        }
    }

    /// Built-in default (monokai, default font): tests and fallbacks.
    pub fn default_dark() -> Theme {
        let a = Appearance::default();
        Theme::new(
            &a,
            schemes::builtin_scheme(&a.color_scheme)
                .cloned()
                .expect("default scheme is built in"),
        )
    }

    pub fn text(&self) -> Style {
        Style::new(self.colors.foreground, self.colors.surface)
    }

    pub fn muted(&self) -> Style {
        Style::new(self.colors.muted, self.colors.surface)
    }

    pub fn accent(&self) -> Style {
        Style::new(self.colors.accent, self.colors.surface)
    }
}

#[cfg(test)]
mod tests {
    use orchard_grid::Rgba;

    use super::*;

    #[test]
    fn appearance_maps_to_grid_settings() {
        let a = Appearance {
            frame_style: "double".into(),
            font_size: 15,
            ..Appearance::default()
        };
        let t = Theme::new(&a, schemes::builtin_scheme("gruvbox").cloned().unwrap());
        assert_eq!(t.frame, BoxStyle::Double);
        assert_eq!(t.font.size, 15.0);
        assert_eq!(t.colors.surface, Rgba::hex(0x282828));
    }

    #[test]
    fn default_dark_is_monokai() {
        let t = Theme::default_dark();
        assert_eq!(t.scheme.name, schemes::DEFAULT_SCHEME);
        assert_eq!(t.frame, BoxStyle::Rounded);
        assert_eq!(t.text(), Style::new(t.colors.foreground, t.colors.surface));
    }
}
