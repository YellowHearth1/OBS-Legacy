//! Малювання екранів.

mod help;
mod home;
mod intro;
mod lists;
mod report;
mod run;

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};

use crate::app::{App, Screen, Target, ToastKind};
use crate::logo;
use crate::theme::Theme;

pub const MIN_WIDTH: u16 = 64;
pub const MIN_HEIGHT: u16 = 20;

pub fn draw(f: &mut Frame, app: &mut App) {
    app.hits.clear();
    let area = f.area();
    // Суцільне полотно під усім інтерфейсом (див. theme.rs).
    f.render_widget(Block::new().style(app.theme.base()), area);
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let msg = Paragraph::new(vec![
            Line::from(Span::styled("OBS-Legacy", app.theme.title())),
            Line::from(trf!(
                "Збільш вікно термінала: потрібно щонайменше {MIN_WIDTH}×{MIN_HEIGHT}",
                "Make the terminal window bigger: at least {MIN_WIDTH}×{MIN_HEIGHT} is needed"
            )),
            Line::from(trf!("(зараз {}×{})", "(now {}×{})", area.width, area.height)),
        ])
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
        f.render_widget(msg, area.centered_vertically(Constraint::Length(3)));
        return;
    }
    match app.screen {
        Screen::Intro => intro::draw(f, app, area),
        Screen::Home => home::draw(f, app, area),
        Screen::Run => run::draw(f, app, area),
        Screen::Plugins => lists::plugins(f, app, area),
        Screen::Rollback => lists::rollback(f, app, area),
        Screen::Settings => lists::settings(f, app, area),
        Screen::Reports => report::list(f, app, area),
        Screen::Report => report::view(f, app, area),
        Screen::Help => help::draw(f, app, area),
    }
    if app.dialog.is_some() {
        dialog(f, app, area);
    }
}

pub fn spinner(tick: u64) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[(tick as usize) % FRAMES.len()]
}

/// Обрізає рядок до `max` символів, додаючи «…».
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Українська множина: plural(4, "потік", "потоки", "потоків") → "потоки".
pub fn plural<'a>(n: usize, one: &'a str, few: &'a str, many: &'a str) -> &'a str {
    match (n % 10, n % 100) {
        (1, r) if r != 11 => one,
        (2..=4, r) if !(12..=14).contains(&r) => few,
        _ => many,
    }
}

/// Слово поточною мовою для числа `n`: `count_word(5, ["потік", "потоки", "потоків"], ["thread", "threads"])`.
pub fn count_word<'a>(n: usize, uk: [&'a str; 3], en: [&'a str; 2]) -> &'a str {
    if crate::i18n::en() { if n == 1 { en[0] } else { en[1] } } else { plural(n, uk[0], uk[1], uk[2]) }
}

/// Перенесення тексту за словами в рядки шириною до `width` символів.
/// Задовгі слова (шляхи) розрізаються, відступ на початку рядка зберігається
/// і для продовжень.
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let indent = text.chars().take_while(|c| *c == ' ').count();
    if indent > 0 && indent < width / 2 {
        let pad = " ".repeat(indent);
        return wrap_text(text.trim_start(), width - indent).into_iter().map(|r| format!("{pad}{r}")).collect();
    }
    let mut rows: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_len = 0;
    for word in text.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        while word.len() > width {
            if cur_len > 0 {
                rows.push(std::mem::take(&mut cur));
                cur_len = 0;
            }
            rows.push(word.drain(..width).collect());
        }
        if word.is_empty() {
            continue;
        }
        if cur_len > 0 && cur_len + 1 + word.len() > width {
            rows.push(std::mem::take(&mut cur));
            cur_len = 0;
        }
        if cur_len > 0 {
            cur.push(' ');
            cur_len += 1;
        }
        cur.extend(word.iter());
        cur_len += word.len();
    }
    if cur_len > 0 || rows.is_empty() {
        rows.push(cur);
    }
    rows
}

/// Маркований пункт із «висячим» відступом: продовження рядка стоїть під
/// текстом, а не під маркером.
pub fn bullet(marker: Span<'static>, text: &str, width: u16) -> Vec<Line<'static>> {
    hanging(marker, text, Style::new(), width)
}

/// Те саме, що `bullet`, але з власним стилем тексту.
pub fn hanging(marker: Span<'static>, text: &str, style: Style, width: u16) -> Vec<Line<'static>> {
    let indent = marker.width();
    let avail = usize::from(width).saturating_sub(indent).max(12);
    wrap_text(text, avail)
        .into_iter()
        .enumerate()
        .map(|(i, row)| {
            let lead = if i == 0 { marker.clone() } else { Span::raw(" ".repeat(indent)) };
            Line::from(vec![lead, Span::styled(row, style)])
        })
        .collect()
}

/// Перенесення різнокольорового тексту: слова зберігають свій стиль.
pub fn wrap_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Vec<Span<'static>>> {
    fn put(
        lines: &mut Vec<Vec<Span<'static>>>,
        used: &mut usize,
        width: usize,
        space: bool,
        word: String,
        style: Style,
    ) {
        let need = word.chars().count() + usize::from(space && *used > 0);
        if *used > 0 && *used + need > width {
            lines.push(Vec::new());
            *used = 0;
        }
        let text = if space && *used > 0 { format!(" {word}") } else { word };
        *used += text.chars().count();
        if let Some(line) = lines.last_mut() {
            line.push(Span::styled(text, style));
        }
    }
    let width = width.max(8);
    let mut lines: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut used = 0;
    // Пробіл перед наступним словом (може перейти й на наступний span).
    let mut space = false;
    for span in spans {
        let style = span.style;
        let mut word = String::new();
        for c in span.content.chars() {
            if c == ' ' {
                if !word.is_empty() {
                    put(&mut lines, &mut used, width, space, std::mem::take(&mut word), style);
                }
                space = true;
            } else {
                word.push(c);
            }
        }
        if !word.is_empty() {
            put(&mut lines, &mut used, width, space, word, style);
            space = false;
        }
    }
    lines
}

pub fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Панель із заголовком; `focus` підсвічує рамку акцентом.
pub fn panel<'a>(theme: &Theme, title: impl Into<Line<'a>>, focus: bool) -> Block<'a> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(if focus { theme.focus_border() } else { theme.border() })
        .style(theme.panel())
        .title(title.into())
}

/// Заголовок панелі: « Назва ».
pub fn panel_title<'a>(theme: &Theme, text: impl Into<String>) -> Line<'a> {
    Line::from(Span::styled(format!(" {} ", text.into()), theme.title()))
}

/// Рядок «назва   значення» для панелей стану.
pub fn kv<'a>(theme: &Theme, label: &'a str, value: Vec<Span<'a>>) -> Line<'a> {
    let mut spans = vec![Span::styled(format!(" {label:<15}"), theme.muted())];
    spans.extend(value);
    Line::from(spans)
}

pub fn section<'a>(theme: &Theme, text: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(" ▍", Style::new().fg(theme.accent)),
        Span::styled(text, Style::new().fg(theme.accent2).add_modifier(Modifier::BOLD)),
    ])
}

/// Заливає прямокутник стилем (замість `Clear`, який робить «дірку» до тла
/// термінала).
pub fn fill(f: &mut Frame, area: Rect, style: Style) {
    f.render_widget(Clear, area);
    f.render_widget(Block::new().style(style), area);
}

/// Верхня смуга кожного екрана: знак OBS-Legacy, «хлібні крихти» і праворуч
/// примітка (версія або шлях).
pub fn top_bar(f: &mut Frame, app: &App, area: Rect, crumbs: &[&str], right: &str) {
    let t = app.theme;
    f.render_widget(Block::new().style(t.bar()), area);
    let mut line = Line::from(Span::raw(" "));
    for span in logo::compact(&t).spans {
        line.push_span(span);
    }
    for crumb in crumbs {
        line.push_span(Span::styled("  ›  ", t.muted()));
        line.push_span(Span::styled(crumb.to_string(), t.strong()));
    }
    let used = line.width() as u16;
    f.render_widget(Paragraph::new(line), area);
    let right = format!("{right} ");
    let right_w = Line::from(right.as_str()).width() as u16;
    if used + right_w + 2 <= area.width {
        f.render_widget(Paragraph::new(Span::styled(right, t.muted())).alignment(Alignment::Right), area);
    }
}

/// Нижня смуга: підказки клавіш ліворуч, сповіщення праворуч.
pub fn footer(f: &mut Frame, app: &mut App, area: Rect, hints: &[(&str, &str)], back: bool) {
    let t = app.theme;
    f.render_widget(Block::new().style(t.bar()), area);
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    for (i, (key, what)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   "));
        }
        if !key.is_empty() {
            spans.push(Span::styled(*key, t.key()));
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(*what, t.muted()));
    }
    let hints_line = Line::from(spans);
    let hints_width = hints_line.width() as u16;
    f.render_widget(Paragraph::new(hints_line), area);
    if back {
        // Клацабельна підказка «назад» — перші символи рядка.
        app.hits.push((Rect { width: hints_width.min(area.width), ..area }, Target::Back));
    }
    if let Some(toast) = &app.toast {
        let (icon, style) = match toast.kind {
            ToastKind::Info => ("•", Style::new().fg(t.accent2)),
            ToastKind::Ok => ("✓", t.ok()),
            ToastKind::Warn => ("⚠", t.warn()),
            ToastKind::Err => ("✗", t.err()),
        };
        let text = format!(" {icon} {} ", toast.text);
        let w = (Line::from(text.as_str()).width() as u16).min(area.width);
        // Не вміщається поруч із підказками — займає весь рядок.
        let r = if hints_width + w + 2 <= area.width { Rect { x: area.right() - w, width: w, ..area } } else { area };
        fill(f, r, t.bar());
        f.render_widget(Paragraph::new(Span::styled(text, style.add_modifier(Modifier::BOLD))), r);
    }
}

pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    area.centered(Constraint::Length(width.min(area.width)), Constraint::Length(height.min(area.height)))
}

/// Кнопка « Enter  Текст ».
pub fn button<'a>(theme: &Theme, key: &'a str, text: &'a str, primary: bool) -> Line<'a> {
    let key_style = if primary { theme.selected() } else { theme.key().add_modifier(Modifier::REVERSED) };
    Line::from(vec![
        Span::styled(format!(" {key} "), key_style),
        Span::styled(format!(" {text} "), if primary { theme.title() } else { theme.muted() }),
    ])
}

/// Ряд кнопок, вирівняний по центру або ліворуч; повертає області кнопок.
pub fn buttons(
    f: &mut Frame,
    app: &mut App,
    area: Rect,
    items: &[(&str, &str, bool, Target)],
    center: bool,
) -> Vec<Rect> {
    let t = app.theme;
    let lines: Vec<Line> = items.iter().map(|(k, txt, primary, _)| button(&t, k, txt, *primary)).collect();
    let gap = 3;
    let total: u16 = lines.iter().map(|l| l.width() as u16).sum::<u16>() + gap * lines.len().saturating_sub(1) as u16;
    let mut x = if center { area.x + area.width.saturating_sub(total) / 2 } else { area.x };
    let mut rects = Vec::new();
    for (line, (_, _, _, target)) in lines.into_iter().zip(items) {
        let w = (line.width() as u16).min(area.right().saturating_sub(x));
        if w == 0 {
            break;
        }
        let r = Rect { x, width: w, ..area };
        f.render_widget(Paragraph::new(line), r);
        app.hits.push((r, *target));
        rects.push(r);
        x += w + gap;
    }
    rects
}

fn dialog(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(d) = &app.dialog else { return };
    let t = app.theme;
    let width = area.width.saturating_sub(8).min(76);
    let body: Vec<Line> = d.lines.iter().map(|l| Line::from(l.clone())).collect();
    let para = Paragraph::new(body).wrap(Wrap { trim: true });
    let inner_w = width.saturating_sub(4);
    let text_h = para.line_count(inner_w) as u16;
    let height = (text_h + 6).min(area.height);
    let r = centered(area, width, height);
    fill(f, r, t.panel());
    let color = if d.danger { t.err } else { t.accent };
    let title = format!(" {} ", d.title);
    let confirm = d.confirm;
    let alt = d.alt.as_ref().map(|(label, _)| *label);
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(color))
        .style(t.panel())
        .title(Line::from(Span::styled(title, Style::new().fg(color).add_modifier(Modifier::BOLD))));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let [text_area, _, buttons_r] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)])
            .areas(inner.inner(ratatui::layout::Margin::new(1, 0)));
    f.render_widget(para, text_area);
    let mut items = vec![("Enter", confirm, true, Target::DialogOk)];
    if let Some(label) = alt {
        items.push(("R", label, false, Target::DialogAlt));
    }
    items.push(("Esc", tr!("Скасувати", "Cancel"), false, Target::DialogCancel));
    buttons(f, app, buttons_r, &items, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_words_and_long_paths() {
        assert_eq!(wrap_text("один два три", 8), ["один два", "три"]);
        assert_eq!(wrap_text("", 10), [""]);
        let rows = wrap_text("шлях: /home/user/.cache/obs-studio-nvenc-artix/reports/x.txt", 20);
        assert!(rows.iter().all(|r| r.chars().count() <= 20), "{rows:?}");
        assert_eq!(rows.concat().replace(' ', ""), "шлях:/home/user/.cache/obs-studio-nvenc-artix/reports/x.txt");
        assert_eq!(wrap_text("  ОС: Artix Linux", 12), ["  ОС: Artix", "  Linux"]);
    }

    #[test]
    fn wraps_styled_spans() {
        let spans =
            vec![Span::raw("obs-multi-rtmp"), Span::styled(" 0.8.0.0", Style::new().bold()), Span::raw(", waveform")];
        let lines = wrap_spans(spans, 24);
        let text: Vec<String> =
            lines.iter().map(|l| l.iter().map(|s| s.content.to_string()).collect::<String>()).collect();
        assert_eq!(text, ["obs-multi-rtmp 0.8.0.0,", "waveform"]);
        assert!(lines[0][1].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn ukrainian_plural() {
        assert_eq!(plural(1, "звіт", "звіти", "звітів"), "звіт");
        assert_eq!(plural(3, "звіт", "звіти", "звітів"), "звіти");
        assert_eq!(plural(11, "звіт", "звіти", "звітів"), "звітів");
        assert_eq!(plural(25, "звіт", "звіти", "звітів"), "звітів");
    }
}
