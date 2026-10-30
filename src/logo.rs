//! Великий логотип «OBS LEGACY» (шрифт ANSI Shadow) з анімованим градієнтом.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;

const GLYPH_O: [&str; 6] = [" ██████╗ ", "██╔═══██╗", "██║   ██║", "██║   ██║", "╚██████╔╝", " ╚═════╝ "];
const GLYPH_B: [&str; 6] = ["██████╗ ", "██╔══██╗", "██████╔╝", "██╔══██╗", "██████╔╝", "╚═════╝ "];
const GLYPH_S: [&str; 6] = ["███████╗", "██╔════╝", "███████╗", "╚════██║", "███████║", "╚══════╝"];
const GLYPH_L: [&str; 6] = ["██╗     ", "██║     ", "██║     ", "██║     ", "███████╗", "╚══════╝"];
const GLYPH_E: [&str; 6] = ["███████╗", "██╔════╝", "█████╗  ", "██╔══╝  ", "███████╗", "╚══════╝"];
const GLYPH_G: [&str; 6] = [" ██████╗ ", "██╔════╝ ", "██║  ███╗", "██║   ██║", "╚██████╔╝", " ╚═════╝ "];
const GLYPH_A: [&str; 6] = [" █████╗ ", "██╔══██╗", "███████║", "██╔══██║", "██║  ██║", "╚═╝  ╚═╝"];
const GLYPH_C: [&str; 6] = [" ██████╗", "██╔════╝", "██║     ", "██║     ", "╚██████╗", " ╚═════╝"];
const GLYPH_Y: [&str; 6] = ["██╗   ██╗", "╚██╗ ██╔╝", " ╚████╔╝ ", "  ╚██╔╝  ", "   ██║   ", "   ╚═╝   "];
const GAP: [&str; 6] = ["   "; 6];

const WORD: [&[&str; 6]; 10] =
    [&GLYPH_O, &GLYPH_B, &GLYPH_S, &GAP, &GLYPH_L, &GLYPH_E, &GLYPH_G, &GLYPH_A, &GLYPH_C, &GLYPH_Y];

pub const HEIGHT: u16 = 6;
pub const WIDTH: u16 = 78;

fn rows() -> [String; 6] {
    std::array::from_fn(|row| WORD.iter().map(|glyph| glyph[row]).collect())
}

/// Рядки логотипа; `phase` зсуває градієнт, створюючи «перелив».
pub fn big(theme: &Theme, phase: f32) -> Vec<Line<'static>> {
    let width = f32::from(WIDTH);
    rows()
        .into_iter()
        .map(|row| {
            let spans: Vec<Span<'static>> = row
                .chars()
                .enumerate()
                .map(|(x, ch)| {
                    // Тіні (╗╔═…) трохи темніші за «цеглини» (█).
                    let wave = ((x as f32 / width) * std::f32::consts::TAU + phase).sin() * 0.5 + 0.5;
                    let mut style = Style::new().fg(theme.gradient(wave));
                    if ch == '█' {
                        style = style.add_modifier(Modifier::BOLD);
                    } else {
                        style = style.add_modifier(Modifier::DIM);
                    }
                    Span::styled(ch.to_string(), style)
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

/// Компактний заголовок для вузьких вікон.
pub fn compact(theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled("▐", Style::new().fg(theme.accent)),
        Span::styled(" OBS ", theme.selected()),
        Span::styled(" LEGACY ", Style::new().fg(theme.on_accent).bg(theme.accent2).add_modifier(Modifier::BOLD)),
        Span::styled("▌", Style::new().fg(theme.accent2)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_rows_have_equal_width() {
        for glyph in WORD {
            let w = glyph[0].chars().count();
            for row in glyph.iter() {
                assert_eq!(row.chars().count(), w, "рядок {row:?}");
            }
        }
    }

    #[test]
    fn logo_width_matches_constant() {
        for row in rows() {
            assert_eq!(row.chars().count(), usize::from(WIDTH));
        }
    }
}
