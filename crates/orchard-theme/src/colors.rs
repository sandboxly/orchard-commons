//! A color scheme resolved to grid colors.

use orchard_grid::Rgba;

use crate::ColorScheme;

/// Every color role of a [`ColorScheme`], parsed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    pub background: Rgba,
    pub surface: Rgba,
    pub foreground: Rgba,
    pub muted: Rgba,
    pub border: Rgba,
    pub border_active: Rgba,
    pub accent: Rgba,
    pub cursor_bg: Rgba,
    pub cursor_fg: Rgba,
    pub cursor_inactive_bg: Rgba,
    pub dir: Rgba,
    pub exec: Rgba,
    pub link: Rgba,
    pub hidden: Rgba,
    pub marked: Rgba,
    pub error: Rgba,
    pub warning: Rgba,
    pub success: Rgba,
    pub footer_bg: Rgba,
    pub footer_fg: Rgba,
    pub footer_key: Rgba,
}

impl Colors {
    pub fn from_scheme(s: &ColorScheme) -> Colors {
        let c = |v: &str, fallback: Rgba| Rgba::parse(v).unwrap_or(fallback);
        let fg = c(&s.foreground, Rgba::WHITE);
        let bg = c(&s.background, Rgba::BLACK);
        Colors {
            background: bg,
            surface: c(&s.surface, bg),
            foreground: fg,
            muted: c(&s.muted, fg),
            border: c(&s.border, fg),
            border_active: c(&s.border_active, fg),
            accent: c(&s.accent, fg),
            cursor_bg: c(&s.cursor_bg, fg),
            cursor_fg: c(&s.cursor_fg, bg),
            cursor_inactive_bg: c(&s.cursor_inactive_bg, bg),
            dir: c(&s.dir, fg),
            exec: c(&s.exec, fg),
            link: c(&s.link, fg),
            hidden: c(&s.hidden, fg),
            marked: c(&s.marked, fg),
            error: c(&s.error, fg),
            warning: c(&s.warning, fg),
            success: c(&s.success, fg),
            footer_bg: c(&s.footer_bg, bg),
            footer_fg: c(&s.footer_fg, fg),
            footer_key: c(&s.footer_key, fg),
        }
    }
}

/// Mix two colors: `t` = 0 is `a`, 1 is `b`.
pub fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Rgba::new(l(a.r, b.r), l(a.g, b.g), l(a.b, b.b), l(a.a, b.a))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_ends() {
        assert_eq!(mix(Rgba::BLACK, Rgba::WHITE, 0.0), Rgba::BLACK);
        assert_eq!(mix(Rgba::BLACK, Rgba::WHITE, 1.0), Rgba::WHITE);
    }

    #[test]
    fn unparsable_colors_fall_back_to_text_and_background() {
        let mut s = crate::schemes::builtin_scheme("nord").unwrap().clone();
        s.surface = "nope".into();
        s.cursor_fg = String::new();
        s.exec = "red".into();
        let c = Colors::from_scheme(&s);
        assert_eq!(c.surface, Rgba::hex(0x242933)); // background
        assert_eq!(c.cursor_fg, Rgba::hex(0x242933));
        assert_eq!(c.exec, Rgba::hex(0xd8dee9)); // foreground
        assert_eq!(c.hidden, Rgba::hex(0x616e88));
    }
}
