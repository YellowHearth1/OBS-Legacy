//! Головний екран: коротке меню ліворуч, праворуч — «Що далі» (одна порада
//! з кнопкою) і «Твоя система» (коротко або докладно — клавіша d, див. system_panel.rs).

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::system_panel::draw_system;
use super::{footer, hanging, lower_first, panel, panel_title, spinner, top_bar};
use crate::advice::{Advice, Stage, Tone};
use crate::app::{App, MenuItem, Row, Target, menu_item};
use crate::logo;
use crate::reports;
use crate::script;
use crate::theme::Theme;

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme;
    let version = trf!("v{} · скрипт {}", "v{} · script {}", env!("CARGO_PKG_VERSION"), script::script_version());
    // Великий логотип — лише коли місця вдосталь (на першому екрані він є завжди).
    let big = area.height >= 44 && area.width >= logo::WIDTH + 4;
    let logo_h = if big { logo::HEIGHT + 1 } else { 0 };
    let [bar, logo_r, _, body, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(logo_h),
        Constraint::Length(1),
        Constraint::Min(8),
        Constraint::Length(1),
    ])
    .areas(area);
    top_bar(
        f,
        app,
        bar,
        &[tr!("OBS Studio з NVENC для NVIDIA 580xx", "OBS Studio with NVENC for NVIDIA 580xx")],
        &version,
    );
    if big {
        let r = logo_r.centered_horizontally(Constraint::Length(logo::WIDTH));
        f.render_widget(Paragraph::new(logo::big(&t, app.tick as f32 * 0.04)), Rect { height: logo::HEIGHT, ..r });
    }

    let body = body.inner(ratatui::layout::Margin::new(1, 0));
    // Меню вужче у вузькому вікні, щоб праворуч лишилося місце для поради.
    let left_w = if body.width >= 100 { 36 } else { 34 }.min(body.width / 2);
    let [left, right] = Layout::horizontal([Constraint::Length(left_w), Constraint::Min(20)]).spacing(1).areas(body);

    let rows = app.rows();
    app.menu_sel = app.menu_sel.min(rows.len() - 1);
    // Пункти + два роздільники + рамка.
    let menu_h = (rows.len() as u16 + 4).min(left.height.saturating_sub(5));
    let [menu_r, about_r] = Layout::vertical([Constraint::Length(menu_h), Constraint::Min(5)]).areas(left);
    let advice = app.advice();
    draw_menu(f, app, menu_r, &rows, advice.as_ref());
    draw_about(f, app, about_r, rows[app.menu_sel]);

    // Порада — головне на екрані, тож вона отримує стільки місця, скільки треба.
    let advice_h = match &advice {
        Some(a) => advice_height(app, a, &rows, right.width),
        None => 4,
    };
    let [advice_r, sys_r] =
        Layout::vertical([Constraint::Length(advice_h.min(right.height.saturating_sub(4))), Constraint::Min(4)])
            .areas(right);
    draw_advice(f, app, advice_r, advice.as_ref(), &rows);
    draw_system(f, app, sys_r);

    footer(
        f,
        app,
        foot,
        &[
            ("↑↓", tr!("вибір", "select")),
            ("Enter", tr!("відкрити", "open")),
            ("o", tr!("звіт", "report")),
            ("d", tr!("подробиці", "details")),
            ("?", tr!("довідка", "help")),
            ("q", tr!("вихід", "quit")),
        ],
        false,
    );
}

fn draw_menu(f: &mut Frame, app: &mut App, area: Rect, rows: &[Row], advice: Option<&Advice>) {
    let t = app.theme;
    let block = panel(&t, panel_title(&t, tr!("Меню", "Menu")), true);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let recommended = advice.and_then(|a| a.action);
    let selected_plugins = app.settings.plugins().len();
    let mut y = inner.y;
    for (i, row) in rows.iter().enumerate() {
        // Роздільники перед «Додатково» і перед довідкою.
        let separator = matches!(row, Row::More) || (i > 0 && *row == Row::Item(crate::app::Action::Help));
        if separator && y < inner.bottom() {
            let line = "─".repeat(usize::from(inner.width.saturating_sub(2)));
            f.render_widget(
                Paragraph::new(Span::styled(format!(" {line}"), t.border())),
                Rect { y, height: 1, ..inner },
            );
            y += 1;
        }
        if y >= inner.bottom() {
            break;
        }
        let r = Rect { y, height: 1, ..inner };
        y += 1;
        let selected = i == app.menu_sel;
        let base = if selected { t.selected() } else { Style::new() };
        let mut spans: Vec<Span> = Vec::new();
        let mut right: Vec<Span> = Vec::new();
        match row {
            Row::Item(action) => {
                let item: &MenuItem = menu_item(*action);
                // Піднятий у основні кроки пункт («Встановити пакети») — без відступу.
                let nested = item.advanced && !(*action == crate::app::Action::Deps && app.deps_promoted());
                let indent = if nested { "   " } else { " " };
                let icon_style = if selected { base } else { Style::new().fg(t.accent2) };
                let danger = *action == crate::app::Action::Uninstall;
                let label_style = match (selected, danger) {
                    (true, _) => base,
                    (false, true) => t.err(),
                    (false, false) if !nested => t.strong(),
                    _ => Style::new(),
                };
                spans.push(Span::styled(format!("{indent}{} ", if selected { "▸" } else { " " }), base));
                spans.push(Span::styled(item.icon, icon_style));
                spans.push(Span::styled(format!("  {}", item.label()), label_style));
                if *action == crate::app::Action::Plugins {
                    right.push(Span::styled(selected_plugins.to_string(), if selected { base } else { t.muted() }));
                }
                if recommended == Some(*action) {
                    right.push(Span::styled("◆", if selected { base } else { t.warn() }));
                }
            }
            Row::More => {
                spans.push(Span::styled(format!(" {} ", if selected { "▸" } else { " " }), base));
                spans.push(Span::styled("⋯", if selected { base } else { Style::new().fg(t.accent2) }));
                spans.push(Span::styled(tr!("  Додатково", "  More"), if selected { base } else { t.muted() }));
                right.push(Span::styled(
                    if app.advanced_open { "▾" } else { "▸" },
                    if selected { base } else { t.muted() },
                ));
            }
        }
        // Позначки праворуч: від тексту — щонайменше пробіл, від рамки — пробіл, якщо влазить.
        let left_w = Line::from(spans.clone()).width();
        let mut right_w = Line::from(right.clone()).width();
        if !right.is_empty() && left_w + right_w + 2 <= usize::from(r.width) {
            right.push(Span::styled(" ", base));
            right_w += 1;
        }
        let pad = usize::from(r.width).saturating_sub(left_w + right_w).max(usize::from(!right.is_empty()));
        spans.push(Span::styled(" ".repeat(pad), base));
        spans.extend(right);
        f.render_widget(Paragraph::new(Line::from(spans)), r);
        app.hits.push((r, Target::Menu(i)));
    }
}

fn draw_about(f: &mut Frame, app: &App, area: Rect, row: Row) {
    let t = app.theme;
    // Рамка + відступ 1 з кожного боку.
    let width = area.width.saturating_sub(4);
    let fact = |label: &str, value: &str, style: Style| {
        hanging(Span::styled(format!("{label:<12}"), t.muted()), value, style, width)
    };
    let (title, mut lines): (String, Vec<Line>) = match row {
        Row::Item(action) => {
            let item = menu_item(action);
            let mut lines = hanging(Span::raw(""), item.about(), Style::new(), width);
            if !matches!(action, crate::app::Action::Help | crate::app::Action::Quit) {
                lines.push(Line::from(""));
                if !item.duration().is_empty() {
                    lines.extend(fact(tr!("Час", "Time"), item.duration(), Style::new()));
                }
                lines.extend(if item.sudo {
                    fact(tr!("Пароль", "Password"), tr!("знадобиться", "needed"), t.warn())
                } else {
                    fact(tr!("Пароль", "Password"), tr!("не потрібен", "not needed"), Style::new())
                });
                lines.extend(if item.changes_system {
                    fact(tr!("Система", "System"), tr!("змінюється", "is changed"), t.warn())
                } else {
                    fact(tr!("Система", "System"), tr!("не змінюється", "stays as is"), t.ok())
                });
            }
            (item.label().to_string(), lines)
        }
        Row::More => (
            tr!("Додатково", "More").to_string(),
            hanging(
                Span::raw(""),
                tr!(
                    "Інструменти, які новачкові зазвичай не потрібні: докладний стан, звіти, відкат, налаштування, \
                     очищення кешу й видалення.",
                    "Tools a beginner usually doesn't need: detailed status, reports, rollback, settings, cleaning \
                     the cache and removal."
                ),
                Style::new(),
                width,
            ),
        ),
    };
    lines.push(Line::from(""));
    lines.extend(hanging(
        Span::raw(""),
        tr!("Миша: клац — вибрати, ще клац — відкрити.", "Mouse: click to select, click again to open."),
        t.muted(),
        width,
    ));
    let para = Paragraph::new(lines)
        .block(panel(&t, panel_title(&t, title), false).padding(ratatui::widgets::Padding::horizontal(1)));
    f.render_widget(para, area);
}

fn tone_style(t: &Theme, tone: Tone) -> (Style, &'static str) {
    match tone {
        Tone::Ok => (t.ok().add_modifier(Modifier::BOLD), "✓"),
        Tone::Info => (Style::new().fg(t.accent2).add_modifier(Modifier::BOLD), "→"),
        Tone::Warn => (t.warn().add_modifier(Modifier::BOLD), "⚠"),
        Tone::Err => (t.err().add_modifier(Modifier::BOLD), "✗"),
    }
}

/// Шлях новачка: «✓ Перевірка ─── ● Встановлення ─── ○ Запуск».
fn tracker(t: &Theme, stage: Stage, width: u16) -> Line<'static> {
    let steps = [
        (Stage::Check, tr!("Перевірка", "Check")),
        (Stage::Install, tr!("Встановлення", "Install")),
        (Stage::Use, tr!("Запуск", "Launch")),
    ];
    let sep = if width >= 52 { " ─── " } else { " › " };
    let mut spans = vec![Span::raw(" ")];
    for (i, (s, name)) in steps.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(sep, t.border()));
        }
        let (icon, style) = if *s < stage {
            ("✓", t.ok())
        } else if *s == stage {
            ("●", t.title())
        } else {
            ("○", t.muted())
        };
        spans.push(Span::styled(format!("{icon} "), style));
        spans.push(Span::styled(name.to_string(), if *s == stage { t.strong() } else { t.muted() }));
    }
    Line::from(spans)
}

/// Текст поради (без кнопок); `width` — ширина для перенесення.
fn advice_lines(t: &Theme, a: &Advice, width: u16, reports: &[reports::ReportInfo]) -> Vec<Line<'static>> {
    let (style, icon) = tone_style(t, a.tone);
    let mut lines = vec![tracker(t, a.stage, width), Line::from("")];
    lines.extend(hanging(Span::styled(format!(" {icon} "), style), &a.title, style, width));
    if let Some(d) = &a.detail {
        lines.extend(hanging(Span::raw("   "), d, Style::new(), width));
    }
    if !a.todo.is_empty() {
        lines.push(Line::from(Span::styled(tr!("   Що робити:", "   What to do:"), t.muted())));
        for item in &a.todo {
            lines.extend(hanging(Span::styled("   → ", Style::new().fg(t.accent2)), item, Style::new(), width));
        }
    }
    if let Some(path) = &a.report
        && let Some(r) = reports.iter().find(|r| &r.path == path)
    {
        let text = trf!("Звіт: {} · {}", "Report: {} · {}", lower_first(r.label()), reports::when(&r.stamp));
        lines.extend(hanging(Span::raw("   "), &text, t.muted(), width));
    }
    lines
}

/// Кнопки поради: (клавіша, підпис, головна, ціль).
fn advice_buttons(app: &App, a: &Advice, rows: &[Row]) -> Vec<(&'static str, &'static str, bool, Target)> {
    let mut items = Vec::new();
    // Enter запускає вибраний пункт меню; коли це і є порада — так і пишемо.
    let selected_is_advice = a.action.is_some_and(|act| rows.get(app.menu_sel) == Some(&Row::Item(act)));
    if let Some(action) = a.action {
        items.push((
            if selected_is_advice { "Enter" } else { "▶" },
            menu_item(action).label(),
            true,
            Target::AdviceAction,
        ));
    }
    if let Some(action) = a.alt {
        items.push(("▶", menu_item(action).label(), false, Target::AdviceAlt));
    }
    if a.report.is_some() {
        items.push(("o", tr!("Переглянути звіт", "View report"), false, Target::OpenReport));
    }
    items
}

/// Скільки рядків займуть кнопки: одна смуга або кожна окремо, якщо не влазять.
fn buttons_height(t: &Theme, items: &[(&str, &str, bool, Target)], width: u16) -> u16 {
    let total: usize = items.iter().map(|(k, txt, p, _)| super::button(t, k, txt, *p).width() + 3).sum();
    if total <= usize::from(width) + 3 { 1 } else { items.len().max(1) as u16 }
}

/// Висота панелі «Що далі» для заданої ширини панелі.
fn advice_height(app: &App, a: &Advice, rows: &[Row], width: u16) -> u16 {
    let t = app.theme;
    let inner_w = width.saturating_sub(2);
    let text = advice_lines(&t, a, inner_w.saturating_sub(2), &app.reports).len() as u16;
    // Рамка (2) + порожній рядок + кнопки.
    text + 3 + buttons_height(&t, &advice_buttons(app, a, rows), inner_w.saturating_sub(2))
}

fn draw_advice(f: &mut Frame, app: &mut App, area: Rect, advice: Option<&Advice>, rows: &[Row]) {
    let t = app.theme;
    let block = panel(&t, panel_title(&t, tr!("Що далі", "What next")), false);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let Some(a) = advice else {
        let msg = Line::from(vec![
            Span::styled(format!(" {} ", spinner(app.tick)), Style::new().fg(t.accent2)),
            Span::raw(tr!("Дивлюся, що вже готово…", "Looking at what is ready…")),
        ]);
        f.render_widget(Paragraph::new(msg), inner);
        return;
    };
    let items = advice_buttons(app, a, rows);
    let buttons_area_w = inner.width.saturating_sub(2);
    let buttons_h = buttons_height(&t, &items, buttons_area_w);
    let lines = advice_lines(&t, a, inner.width.saturating_sub(2), &app.reports);
    let text_h = (lines.len() as u16).min(inner.height.saturating_sub(buttons_h + 1));
    let [text_r, _, buttons_r] =
        Layout::vertical([Constraint::Length(text_h), Constraint::Length(1), Constraint::Length(buttons_h)])
            .areas(inner);
    f.render_widget(Paragraph::new(lines), text_r);
    let area = buttons_r.inner(ratatui::layout::Margin::new(1, 0));
    if buttons_h == 1 {
        super::buttons(f, app, area, &items, false);
    } else {
        for (i, item) in items.iter().enumerate() {
            let row = Rect { y: area.y + i as u16, height: 1, ..area };
            if row.y < area.bottom() {
                super::buttons(f, app, row, std::slice::from_ref(item), false);
            }
        }
    }
}
