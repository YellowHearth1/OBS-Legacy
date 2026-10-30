//! Віджет, що малює екран емулятора vt100 — живий вивід скрипта з кольорами.
//! Типові кольори й 16 базових кольорів ANSI беруться з теми, тож вивід
//! читається на суцільному фоні так само, як решта інтерфейсу.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::theme::Theme;

pub struct TermView<'a> {
    screen: &'a vt100::Screen,
    theme: &'a Theme,
}

impl<'a> TermView<'a> {
    pub fn new(screen: &'a vt100::Screen, theme: &'a Theme) -> Self {
        Self { screen, theme }
    }
}

fn cell_style(cell: &vt100::Cell, theme: &Theme) -> Style {
    let mut style = Style::new().fg(theme.term_color(cell.fgcolor(), false)).bg(theme.term_color(cell.bgcolor(), true));
    let mut m = Modifier::empty();
    if cell.bold() {
        m |= Modifier::BOLD;
    }
    if cell.dim() {
        m |= Modifier::DIM;
    }
    if cell.italic() {
        m |= Modifier::ITALIC;
    }
    if cell.underline() {
        m |= Modifier::UNDERLINED;
    }
    if cell.inverse() {
        m |= Modifier::REVERSED;
    }
    style = style.add_modifier(m);
    style
}

impl Widget for TermView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (rows, cols) = self.screen.size();
        for row in 0..area.height.min(rows.get()) {
            let mut spans: Vec<Span<'static>> = Vec::new();
            let mut text = String::new();
            let mut style = Style::new();
            for col in 0..area.width.min(cols.get()) {
                let Some(cell) = self.screen.cell(row, col) else { continue };
                if cell.is_wide_continuation() {
                    continue;
                }
                let s = cell_style(cell, self.theme);
                if s != style && !text.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut text), style));
                }
                style = s;
                text.push_str(if cell.has_contents() { cell.contents() } else { " " });
            }
            if !text.is_empty() {
                spans.push(Span::styled(text, style));
            }
            buf.set_line(area.x, area.y + row, &Line::from(spans), area.width);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ColorMode;
    use ratatui::style::Color;
    use std::num::NonZeroU16;

    #[test]
    fn renders_colored_text() {
        let mut p = vt100::Parser::new(NonZeroU16::new(3).unwrap(), NonZeroU16::new(20).unwrap(), 0);
        p.process("\x1b[32m[OK]\x1b[0m готово\r\nдругий".as_bytes());
        let area = Rect::new(0, 0, 20, 3);
        let mut buf = Buffer::empty(area);
        let theme = Theme::for_mode(ColorMode::Indexed, true);
        TermView::new(p.screen(), &theme).render(area, &mut buf);
        let row0: String = (0..20).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert!(row0.starts_with("[OK] готово"));
        assert_eq!(buf[(1, 0)].fg, Color::Indexed(2));
        // Типовий фон — фон панелі, а не «дірка» до тла термінала.
        assert_eq!(buf[(1, 0)].bg, theme.surface);
        assert_eq!(buf[(6, 0)].fg, theme.fg);
        let row1: String = (0..6).map(|x| buf[(x, 1)].symbol().to_string()).collect();
        assert_eq!(row1, "другий");
    }
}
