//! Кольори інтерфейсу. Акцент — фірмовий зелений NVIDIA, другий колір —
//! бірюзовий. Палітра підлаштовується під можливості термінала.
//!
//! Суцільний фон (типово увімкнено) малює власне темне полотно під усім
//! інтерфейсом: у прозорих терміналах (Kitty, Alacritty, Konsole…) клітинки з
//! явним кольором фону лишаються непрозорими, тож текст не губиться на тлі
//! робочого столу. Вимкнений фон — колір термінала, як було раніше.

use std::env;

use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    /// 24-бітні кольори (COLORTERM=truecolor).
    TrueColor,
    /// 256 кольорів.
    Indexed,
    /// 16 базових кольорів.
    Basic,
    /// Без кольорів (NO_COLOR).
    Mono,
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub mode: ColorMode,
    /// Чи малюється власний фон.
    pub solid: bool,
    /// Полотно під усім інтерфейсом.
    pub bg: Color,
    /// Фон панелей — на півтону світліший за полотно.
    pub surface: Color,
    /// Верхня й нижня смуги, неактивні кнопки.
    pub raised: Color,
    /// Основний текст.
    pub fg: Color,
    pub accent: Color,
    pub accent2: Color,
    pub ok: Color,
    pub warn: Color,
    pub err: Color,
    pub muted: Color,
    pub border: Color,
    pub on_accent: Color,
}

impl Theme {
    pub fn detect(solid: bool) -> Self {
        if env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            return Self::for_mode(ColorMode::Mono, solid);
        }
        let colorterm = env::var("COLORTERM").unwrap_or_default().to_lowercase();
        let term = env::var("TERM").unwrap_or_default();
        let mode = if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            ColorMode::TrueColor
        } else if term.contains("256") || term.contains("kitty") || term.contains("alacritty") {
            ColorMode::Indexed
        } else {
            ColorMode::Basic
        };
        Self::for_mode(mode, solid)
    }

    pub fn for_mode(mode: ColorMode, solid: bool) -> Self {
        let mut t = match mode {
            // Полотно — темний графіт із ледь зеленим відтінком, як текстоліт
            // відеокарти. Контраст тексту з ним — понад 4.5:1 (WCAG AA).
            ColorMode::TrueColor => Self {
                mode,
                solid,
                bg: Color::Rgb(0x0F, 0x14, 0x11),
                surface: Color::Rgb(0x15, 0x1B, 0x17),
                raised: Color::Rgb(0x1E, 0x26, 0x21),
                fg: Color::Rgb(0xE3, 0xE8, 0xE1),
                accent: Color::Rgb(0x76, 0xB9, 0x00),
                accent2: Color::Rgb(0x00, 0xC8, 0xD7),
                ok: Color::Rgb(0x8C, 0xD9, 0x2F),
                warn: Color::Rgb(0xFF, 0xC4, 0x2E),
                err: Color::Rgb(0xFF, 0x6B, 0x6B),
                muted: Color::Rgb(0x8E, 0x98, 0x91),
                border: Color::Rgb(0x34, 0x3F, 0x38),
                on_accent: Color::Rgb(0x0B, 0x0F, 0x08),
            },
            ColorMode::Indexed => Self {
                mode,
                solid,
                bg: Color::Indexed(233),
                surface: Color::Indexed(234),
                raised: Color::Indexed(236),
                fg: Color::Indexed(253),
                accent: Color::Indexed(112),
                accent2: Color::Indexed(44),
                ok: Color::Indexed(113),
                warn: Color::Indexed(220),
                err: Color::Indexed(203),
                muted: Color::Indexed(246),
                border: Color::Indexed(240),
                on_accent: Color::Indexed(16),
            },
            ColorMode::Basic => Self {
                mode,
                solid,
                bg: Color::Black,
                surface: Color::Black,
                raised: Color::Black,
                fg: Color::White,
                accent: Color::Green,
                accent2: Color::Cyan,
                ok: Color::Green,
                warn: Color::Yellow,
                err: Color::Red,
                muted: Color::Gray,
                border: Color::DarkGray,
                on_accent: Color::Black,
            },
            ColorMode::Mono => Self {
                mode,
                solid,
                bg: Color::Reset,
                surface: Color::Reset,
                raised: Color::Reset,
                fg: Color::Reset,
                accent: Color::Reset,
                accent2: Color::Reset,
                ok: Color::Reset,
                warn: Color::Reset,
                err: Color::Reset,
                muted: Color::Reset,
                border: Color::Reset,
                on_accent: Color::Reset,
            },
        };
        if !solid {
            t.bg = Color::Reset;
            t.surface = Color::Reset;
            t.raised = Color::Reset;
            t.fg = Color::Reset;
        }
        t
    }

    /// Колір градієнта між акцентами; `t` у межах 0..=1.
    pub fn gradient(&self, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        match (self.mode, self.accent, self.accent2) {
            (ColorMode::TrueColor, Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
                let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
                Color::Rgb(mix(r1, r2), mix(g1, g2), mix(b1, b2))
            }
            (ColorMode::Indexed, _, _) => {
                // Зелено-бірюзова гама з кубу 6×6×6.
                const STEPS: [u8; 5] = [112, 77, 42, 43, 44];
                Color::Indexed(STEPS[((t * (STEPS.len() - 1) as f32).round() as usize).min(STEPS.len() - 1)])
            }
            _ if t < 0.5 => self.accent,
            _ => self.accent2,
        }
    }

    /// Полотно: усе, що не накрите панеллю.
    pub fn base(&self) -> Style {
        Style::new().fg(self.fg).bg(self.bg)
    }

    /// Внутрішність панелі.
    pub fn panel(&self) -> Style {
        Style::new().fg(self.fg).bg(self.surface)
    }

    /// Верхня й нижня смуги.
    pub fn bar(&self) -> Style {
        Style::new().fg(self.fg).bg(self.raised)
    }

    pub fn title(&self) -> Style {
        Style::new().fg(self.accent).add_modifier(Modifier::BOLD)
    }

    pub fn strong(&self) -> Style {
        Style::new().fg(self.fg).add_modifier(Modifier::BOLD)
    }

    pub fn muted(&self) -> Style {
        Style::new().fg(self.muted)
    }

    pub fn border(&self) -> Style {
        Style::new().fg(self.border)
    }

    pub fn focus_border(&self) -> Style {
        Style::new().fg(self.accent)
    }

    pub fn selected(&self) -> Style {
        if self.mode == ColorMode::Mono {
            Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::new().fg(self.on_accent).bg(self.accent).add_modifier(Modifier::BOLD)
        }
    }

    pub fn key(&self) -> Style {
        Style::new().fg(self.accent2).add_modifier(Modifier::BOLD)
    }

    pub fn ok(&self) -> Style {
        Style::new().fg(self.ok)
    }

    pub fn warn(&self) -> Style {
        Style::new().fg(self.warn)
    }

    pub fn err(&self) -> Style {
        Style::new().fg(self.err)
    }

    pub fn banner(&self, color: Color) -> Style {
        if self.mode == ColorMode::Mono {
            Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::new().fg(self.on_accent).bg(color).add_modifier(Modifier::BOLD)
        }
    }

    /// Колір вбудованого термінала. 16 базових кольорів ANSI підібрано під
    /// темне полотно (у truecolor), інакше — як у палітрі термінала.
    /// `background` — колір фону клітинки, а не тексту.
    pub fn term_color(&self, c: vt100::Color, background: bool) -> Color {
        match c {
            vt100::Color::Default if background => self.surface,
            vt100::Color::Default => self.fg,
            vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
            vt100::Color::Idx(0) if background && self.solid => self.surface,
            vt100::Color::Idx(i) if self.mode == ColorMode::TrueColor && i < 16 => {
                const ANSI: [(u8, u8, u8); 16] = [
                    (0x34, 0x3F, 0x38),
                    (0xFF, 0x6B, 0x6B),
                    (0x8C, 0xD9, 0x2F),
                    (0xFF, 0xC4, 0x2E),
                    (0x6F, 0xA8, 0xFF),
                    (0xD0, 0x8C, 0xFF),
                    (0x00, 0xC8, 0xD7),
                    (0xC9, 0xCF, 0xC8),
                    (0x8E, 0x98, 0x91),
                    (0xFF, 0x8A, 0x8A),
                    (0xA6, 0xE9, 0x5A),
                    (0xFF, 0xD8, 0x66),
                    (0x8D, 0xBB, 0xFF),
                    (0xE0, 0xA8, 0xFF),
                    (0x5F, 0xE0, 0xEA),
                    (0xF3, 0xF6, 0xF1),
                ];
                let (r, g, b) = ANSI[usize::from(i)];
                Color::Rgb(r, g, b)
            }
            vt100::Color::Idx(i) => Color::Indexed(i),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Відносна яскравість sRGB за WCAG 2.x.
    fn luminance(c: Color) -> f64 {
        let Color::Rgb(r, g, b) = c else { panic!("потрібен RGB") };
        let ch = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)
    }

    fn contrast(a: Color, b: Color) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn text_is_readable_on_every_surface() {
        let t = Theme::for_mode(ColorMode::TrueColor, true);
        for bg in [t.bg, t.surface, t.raised] {
            for fg in [t.fg, t.muted, t.accent, t.accent2, t.ok, t.warn, t.err] {
                assert!(contrast(fg, bg) >= 4.5, "{fg:?} на {bg:?}: {:.2}", contrast(fg, bg));
            }
        }
        assert!(contrast(t.on_accent, t.accent) >= 4.5);
    }

    #[test]
    fn transparent_mode_keeps_terminal_colors() {
        let t = Theme::for_mode(ColorMode::TrueColor, false);
        assert_eq!(t.bg, Color::Reset);
        assert_eq!(t.surface, Color::Reset);
        assert_eq!(t.fg, Color::Reset);
        assert_eq!(t.term_color(vt100::Color::Default, true), Color::Reset);
    }
}
