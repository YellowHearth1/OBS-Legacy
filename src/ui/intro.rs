//! Титульний екран: логотип, обов'язкове попередження про те, що програму
//! створено за допомогою Claude Code, і подяка проєкту-натхненнику.
//! Показується при кожному запуску.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};

use super::{bullet, buttons, centered};
use crate::app::{App, Target};
use crate::logo;
use crate::script;

pub const INSPIRATION_URL: &str = "https://github.com/sl5net/build-obs-nvenc-legacy";
pub const REPO_URL: &str = "https://github.com/YellowHearth1/OBS-Legacy";

/// Текст попередження (той самий показує й довідка); `width` — ширина
/// тексту для перенесення пунктів із відступом.
pub fn notice_lines(t: &crate::theme::Theme, width: u16) -> Vec<Line<'static>> {
    let marker = || Span::styled("  •  ", Style::new().fg(t.warn));
    let mut lines = vec![
        Line::from(Span::styled(
            "Скрипт зроблений за допомогою Claude Code.",
            Style::new().fg(t.warn).add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        Line::from(""),
        Line::from(
            "Цю програму — TUI-оболонку OBS-Legacy і скрипт збирання — створено за допомогою Claude Code, \
             ШІ-асистента для програмування від Anthropic, на замовлення і під керівництвом автора \
             репозиторію YellowHearth1.",
        ),
        Line::from(""),
    ];
    for text in [
        "Код написав штучний інтелект; незалежного аудиту безпеки він не проходив.",
        "Програма збирає OBS Studio, плагіни й ffmpeg з вихідних кодів під твоє залізо, пакує їх у пакети \
         pacman і ставить через pacman (лише після твого підтвердження) за допомогою sudo/doas.",
        "Система змінюється лише через pacman; попередній набір пакетів лишається для відкату.",
        "Почни з «Перевірити систему» — вона нічого не змінює.",
        "Використовуєш на власний ризик, без жодних гарантій (ліцензія Apache-2.0).",
    ] {
        lines.extend(bullet(marker(), text, width));
    }
    lines
}

pub fn credit_lines(t: &crate::theme::Theme) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled("♥ ", Style::new().fg(t.err).add_modifier(Modifier::BOLD)),
            Span::styled("Натхнення: ", t.strong()),
            Span::styled(INSPIRATION_URL, Style::new().fg(t.accent2).add_modifier(Modifier::UNDERLINED)),
        ]),
        Line::from("Якби не цей проєкт, OBS-Legacy не з'явився б. Дякуємо його автору!"),
    ]
}

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme;
    let width = area.width.saturating_sub(4).min(100);
    let inner_w = width.saturating_sub(4);

    let notice = Paragraph::new(notice_lines(&t, inner_w)).wrap(Wrap { trim: false });
    let notice_h = notice.line_count(inner_w) as u16 + 2;
    let credits = Paragraph::new(credit_lines(&t)).wrap(Wrap { trim: false }).alignment(Alignment::Center);
    let credits_lines = credits.line_count(inner_w) as u16;

    // Від найпросторішого варіанта до найщільнішого: логотип → без логотипа →
    // подяка без рамки й без відступів. Попередження завжди повне.
    let roomy = |header: u16, gaps: u16, credit_border: u16| {
        header + 1 + gaps + notice_h + credits_lines + credit_border + gaps + 1
    };
    let big = area.width >= logo::WIDTH + 4 && roomy(logo::HEIGHT + 1, 1, 2) <= area.height;
    let (header_h, gap, credit_border) = if big {
        (logo::HEIGHT + 1, 1, 2)
    } else if roomy(1, 1, 2) <= area.height {
        (1, 1, 2)
    } else {
        (1, 0, 0)
    };
    let credits_h = credits_lines + credit_border;
    let total = roomy(header_h, gap, credit_border);
    let r = centered(area, width, total.min(area.height));

    let [header, subtitle, _, notice_r, credits_r, _, buttons_r] = Layout::vertical([
        Constraint::Length(header_h),
        Constraint::Length(1),
        Constraint::Length(gap),
        Constraint::Length(notice_h),
        Constraint::Length(credits_h),
        Constraint::Length(gap),
        Constraint::Length(1),
    ])
    .areas(r);

    if big {
        let phase = app.tick as f32 * 0.10;
        let logo_r = header.centered_horizontally(Constraint::Length(logo::WIDTH));
        f.render_widget(Paragraph::new(logo::big(&t, phase)), logo_r);
    } else {
        f.render_widget(Paragraph::new(logo::compact(&t)).alignment(Alignment::Center), header);
    }
    let mut sub = vec![
        Span::styled("OBS Studio з NVENC для NVIDIA 580xx", t.strong()),
        Span::styled(format!("  ·  v{}", env!("CARGO_PKG_VERSION")), t.muted()),
    ];
    if subtitle.width >= 90 {
        sub.insert(1, Span::styled("  ·  Artix / Arch Linux", t.muted()));
        sub.push(Span::styled(format!(" (скрипт {})", script::script_version()), t.muted()));
    }
    f.render_widget(Paragraph::new(Line::from(sub)).alignment(Alignment::Center), subtitle);

    let warn_block = Block::bordered()
        .border_type(BorderType::Thick)
        .border_style(Style::new().fg(t.warn))
        .style(t.panel())
        .title(Line::from(Span::styled(
            " ⚠  ВАЖЛИВО — ПРОЧИТАЙ ПЕРЕД ПОЧАТКОМ ",
            Style::new().fg(t.warn).add_modifier(Modifier::BOLD),
        )))
        .title_alignment(Alignment::Center)
        .padding(ratatui::widgets::Padding::horizontal(1));
    f.render_widget(notice.block(warn_block), notice_r);

    if credit_border > 0 {
        let credit_block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(t.border())
            .style(t.panel())
            .padding(ratatui::widgets::Padding::horizontal(1));
        f.render_widget(credits.block(credit_block), credits_r);
    } else {
        f.render_widget(credits, credits_r);
    }

    buttons(
        f,
        app,
        buttons_r,
        &[("Enter", "Зрозуміло, продовжити", true, Target::Continue), ("Q", "Вийти", false, Target::Quit)],
        true,
    );
}
