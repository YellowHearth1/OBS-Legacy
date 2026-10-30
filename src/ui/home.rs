//! Головний екран: коротке меню ліворуч, праворуч — «Що далі» (одна порада
//! з кнопкою) і «Твоя система» (коротко або докладно — клавіша d).

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::{footer, hanging, kv, panel, panel_title, plural, section, spinner, top_bar, truncate, wrap_spans};
use crate::advice::{Advice, Stage, Tone};
use crate::app::{App, MenuItem, Row, Target, menu_item};
use crate::logo;
use crate::reports::{self, Outcome};
use crate::script;
use crate::system;
use crate::theme::Theme;

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme;
    let version = format!("v{} · скрипт {}", env!("CARGO_PKG_VERSION"), script::script_version());
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
    top_bar(f, app, bar, &["OBS Studio з NVENC для NVIDIA 580xx"], &version);
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
        &[("↑↓", "вибір"), ("Enter", "відкрити"), ("o", "звіт"), ("d", "подробиці"), ("?", "довідка"), ("q", "вихід")],
        false,
    );
}

fn draw_menu(f: &mut Frame, app: &mut App, area: Rect, rows: &[Row], advice: Option<&Advice>) {
    let t = app.theme;
    let block = panel(&t, panel_title(&t, "Меню"), true);
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
                spans.push(Span::styled(format!("  {}", item.label), label_style));
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
                spans.push(Span::styled("  Додатково", if selected { base } else { t.muted() }));
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
        hanging(Span::styled(format!("{label:<11}"), t.muted()), value, style, width)
    };
    let (title, mut lines): (String, Vec<Line>) = match row {
        Row::Item(action) => {
            let item = menu_item(action);
            let mut lines = hanging(Span::raw(""), item.about, Style::new(), width);
            if !matches!(action, crate::app::Action::Help | crate::app::Action::Quit) {
                lines.push(Line::from(""));
                if !item.duration.is_empty() {
                    lines.extend(fact("Час", item.duration, Style::new()));
                }
                lines.extend(if item.sudo {
                    fact("Пароль", "знадобиться", t.warn())
                } else {
                    fact("Пароль", "не потрібен", Style::new())
                });
                lines.extend(if item.changes_system {
                    fact("Система", "змінюється", t.warn())
                } else {
                    fact("Система", "не змінюється", t.ok())
                });
            }
            (item.label.to_string(), lines)
        }
        Row::More => (
            "Додатково".to_string(),
            hanging(
                Span::raw(""),
                "Інструменти, які новачкові зазвичай не потрібні: докладний стан, звіти, відкат, налаштування, \
                 очищення кешу й видалення.",
                Style::new(),
                width,
            ),
        ),
    };
    lines.push(Line::from(""));
    lines.extend(hanging(Span::raw(""), "Миша: клац — вибрати, ще клац — відкрити.", t.muted(), width));
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

/// «Встановлення OBS» → «встановлення OBS» (абревіатури лишаються великими).
fn lower_first(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_lowercase().chain(chars).collect()).unwrap_or_default()
}

/// Шлях новачка: «✓ Перевірка ─── ● Встановлення ─── ○ Запуск».
fn tracker(t: &Theme, stage: Stage, width: u16) -> Line<'static> {
    let steps = [(Stage::Check, "Перевірка"), (Stage::Install, "Встановлення"), (Stage::Use, "Запуск")];
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
        lines.push(Line::from(Span::styled("   Що робити:", t.muted())));
        for item in &a.todo {
            lines.extend(hanging(Span::styled("   → ", Style::new().fg(t.accent2)), item, Style::new(), width));
        }
    }
    if let Some(path) = &a.report
        && let Some(r) = reports.iter().find(|r| &r.path == path)
    {
        let text = format!("Звіт: {} · {}", lower_first(r.label()), reports::when(&r.stamp));
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
            menu_item(action).label,
            true,
            Target::AdviceAction,
        ));
    }
    if let Some(action) = a.alt {
        items.push(("▶", menu_item(action).label, false, Target::AdviceAlt));
    }
    if a.report.is_some() {
        items.push(("o", "Переглянути звіт", false, Target::OpenReport));
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
    let block = panel(&t, panel_title(&t, "Що далі"), false);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let Some(a) = advice else {
        let msg = Line::from(vec![
            Span::styled(format!(" {} ", spinner(app.tick)), Style::new().fg(t.accent2)),
            Span::raw("Дивлюся, що вже готово…"),
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

fn draw_system(f: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme;
    let toggle = if app.details { " d — коротко " } else { " d — подробиці " };
    let mut title = vec![Span::styled(" Твоя система ", t.title())];
    if app.sys_loading {
        title.push(Span::styled(format!("{} ", spinner(app.tick)), Style::new().fg(t.accent2)));
    }
    let block = panel(&t, Line::from(title), false)
        .title(Line::from(Span::styled(toggle, t.muted())).alignment(Alignment::Right));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let toggle_w = Line::from(toggle).width() as u16;
    app.hits.push((
        Rect { x: area.right().saturating_sub(toggle_w + 1), y: area.y, width: toggle_w, height: 1 },
        Target::ToggleDetails,
    ));
    let lines = if app.details { dashboard(app) } else { summary(app, inner.width) };
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// Коротко: по рядку на те, що важливо новачкові.
fn summary(app: &App, width: u16) -> Vec<Line<'static>> {
    let t: &Theme = &app.theme;
    let Some(s) = &app.sys else {
        return vec![Line::from(vec![
            Span::styled(format!(" {} ", spinner(app.tick)), Style::new().fg(t.accent2)),
            Span::raw("Збираю відомості про систему…"),
        ])];
    };
    // « ✓ Назва           значення» — довге значення переноситься під себе.
    const LABEL_W: usize = 16;
    let row = |icon: &str, style: Style, label: &str, value: Vec<Span<'static>>| -> Vec<Line<'static>> {
        let avail = usize::from(width).saturating_sub(3 + LABEL_W);
        wrap_spans(value, avail)
            .into_iter()
            .enumerate()
            .map(|(i, mut spans)| {
                let lead = if i == 0 {
                    vec![
                        Span::styled(format!(" {icon} "), style),
                        Span::styled(format!("{label:<LABEL_W$}"), t.muted()),
                    ]
                } else {
                    vec![Span::raw(" ".repeat(3 + LABEL_W))]
                };
                let mut line = lead;
                line.append(&mut spans);
                Line::from(line)
            })
            .collect()
    };
    let ok = |label: &str, v: String| row("✓", t.ok(), label, vec![Span::raw(v)]);
    let mut l: Vec<Line<'static>> = Vec::new();

    match s.gpus.first() {
        Some(g) => l.extend(ok("Відеокарта", g.trim_start_matches("NVIDIA ").to_string())),
        None => l.extend(row("✗", t.err(), "Відеокарта", vec![Span::styled("NVIDIA не знайдено", t.err())])),
    }
    match (s.driver.clone(), s.driver_major()) {
        (Some(v), Some(580)) => l.extend(ok("Драйвер", format!("{v} · гілка 580xx"))),
        (Some(v), Some(m)) if m > 580 => {
            l.extend(row("•", t.muted(), "Драйвер", vec![Span::raw(v), Span::styled(" · новіший за 580xx", t.muted())]))
        }
        (Some(v), _) => l.extend(ok("Драйвер", v)),
        (None, _) => l.extend(row("✗", t.err(), "Драйвер", vec![Span::styled("не завантажено", t.err())])),
    }
    match (s.nvenc_api.clone(), s.nvenc_compatible()) {
        (Some(a), Some(true) | None) => l.extend(ok("NVENC", format!("API {a} — підходить"))),
        (Some(a), Some(false)) => {
            l.extend(row("✗", t.err(), "NVENC", vec![Span::styled(format!("API {a} — замало"), t.err())]))
        }
        (None, _) => l.extend(row("⚠", t.warn(), "NVENC", vec![Span::styled("невідомо", t.warn())])),
    }
    let free = s.free_cache_gib.into_iter().chain(s.free_root_gib).fold(f64::INFINITY, f64::min);
    if free.is_finite() {
        if free < 10.0 {
            l.extend(row(
                "⚠",
                t.warn(),
                "Диск",
                vec![Span::styled(format!("вільно {free:.0} ГіБ — треба ≥ 10"), t.warn())],
            ));
        } else {
            l.extend(ok("Диск", format!("вільно {free:.0} ГіБ")));
        }
    }

    match app.missing_packages.as_deref() {
        Some([]) if app.missing_norepo.is_empty() => l.extend(ok("Пакети", "усі на місці".into())),
        Some(m) if !m.is_empty() => l.extend(row(
            "⚠",
            t.warn(),
            "Пакети",
            vec![Span::styled(format!("бракує {}: {}", m.len(), m.join(", ")), t.warn())],
        )),
        Some(_) => l.extend(row(
            "✗",
            t.err(),
            "Пакети",
            vec![Span::styled(format!("немає в репозиторіях: {}", app.missing_norepo.join(", ")), t.err())],
        )),
        None => {}
    }

    let active = s.active_build();
    match active {
        Some(b) => {
            l.extend(ok("OBS з NVENC", format!("OBS {} · зібрано {}", b.obs_version(), b.built())));
            if b.get("cef_build").is_none_or(str::is_empty) {
                l.extend(row("⚠", t.warn(), "Browser Source", vec![Span::styled("невідомо", t.warn())]));
            }
            match (b.legacy, b.profile()) {
                (true, _) => l.extend(row(
                    "⚠",
                    t.warn(),
                    "Схема",
                    vec![Span::styled("стара (/opt) — онови, щоб отримати пакети pacman", t.warn())],
                )),
                (false, Some(p)) => l.extend(ok("Оптимізовано", p)),
                (false, None) => {}
            }
            if s.obs_cmd_ok() {
                l.extend(ok("Команда obs", "відкриває OBS з NVENC".into()));
            } else {
                l.extend(row(
                    "⚠",
                    t.warn(),
                    "Команда obs",
                    vec![Span::styled("веде не на OBS-Legacy (PATH?)", t.warn())],
                ));
            }
        }
        None => l.extend(row("•", t.muted(), "OBS з NVENC", vec![Span::styled("ще не встановлено", t.muted())])),
    }

    let selected = app.settings.plugins();
    let installed = active.map(|b| b.plugins()).unwrap_or_default();
    let mut plugins: Vec<Span<'static>> = Vec::new();
    if selected.is_empty() && installed.is_empty() {
        plugins.push(Span::styled("без плагінів", t.muted()));
    }
    for (i, id) in selected.iter().enumerate() {
        if i > 0 {
            plugins.push(Span::styled(", ", t.muted()));
        }
        plugins.push(Span::raw(id.clone()));
        match installed.iter().find(|(pid, _)| pid == id) {
            Some((_, v)) => plugins.push(Span::styled(format!(" {v}"), t.muted())),
            None if active.is_some() => plugins.push(Span::styled(" (буде додано)", t.warn())),
            None => {}
        }
    }
    let plugins_icon = if active.is_some() { ("✓", t.ok()) } else { ("•", t.muted()) };
    l.extend(row(plugins_icon.0, plugins_icon.1, "Плагіни", plugins));

    if app.updates_loading {
        l.extend(row(
            spinner(app.tick),
            Style::new().fg(t.accent2),
            "Оновлення",
            vec![Span::styled("перевіряю…", t.muted())],
        ));
    } else if let Some(f) = app.facts()
        && let Some(latest) = f.obs_latest.clone()
    {
        let news = if let Some(l) = f.obs_update() {
            Some(format!("вийшов OBS {l}"))
        } else if !f.plugin_updates.is_empty() {
            Some(format!("нові версії плагінів: {}", f.plugin_updates.len()))
        } else if let Some(v) = &f.ffmpeg_new {
            Some(format!("у репозиторії новий ffmpeg {v} — перезбери"))
        } else {
            f.self_update()
                .filter(|_| app.self_update_src.is_some())
                .map(|l| format!("вийшов OBS-Legacy {}", l.trim_start_matches('v')))
        };
        match (active, news) {
            (Some(_), Some(news)) => l.extend(row("⚠", t.warn(), "Оновлення", vec![Span::styled(news, t.warn())])),
            (Some(b), None) if f.obs_pinned.is_some() && system::newer(&latest, b.obs_version()) => l.extend(row(
                "•",
                t.muted(),
                "Оновлення",
                vec![Span::styled(format!("OBS {} зафіксовано, найновіший — {latest}", b.obs_version()), t.muted())],
            )),
            (Some(b), None) => l.extend(ok("Оновлення", format!("OBS {} — найновіший", b.obs_version()))),
            (None, _) => {
                l.extend(row("•", t.muted(), "Оновлення", vec![Span::raw(format!("найновіший OBS — {latest}"))]))
            }
        }
    }

    if let Some(r) = app.reports.first() {
        let (icon, style, what) = match r.outcome {
            Outcome::Success if r.warnings > 0 => (
                "✓",
                t.ok(),
                format!("готово, {} {}", r.warnings, plural(r.warnings, "зауваження", "зауваження", "зауважень")),
            ),
            Outcome::Success => ("✓", t.ok(), "готово".to_string()),
            Outcome::Failed => ("✗", t.err(), "помилка".to_string()),
            Outcome::Cancelled => ("■", t.warn(), "скасовано".to_string()),
            Outcome::Unknown => ("•", t.muted(), "не завершено".to_string()),
        };
        l.extend(row(
            icon,
            style,
            "Останній запуск",
            vec![
                Span::raw(format!("{} · {}", lower_first(r.label()), reports::when(&r.stamp))),
                Span::styled(format!(" — {what}"), style),
            ],
        ));
    }
    l
}

/// Докладно: усе, що відомо про систему й збірки (для досвідчених).
fn dashboard(app: &App) -> Vec<Line<'static>> {
    let t: &Theme = &app.theme;
    let Some(s) = &app.sys else {
        return vec![Line::from(vec![
            Span::styled(format!(" {} ", spinner(app.tick)), Style::new().fg(t.accent2)),
            Span::raw("Збираю відомості про систему…"),
        ])];
    };
    let ok = |s: String| Span::styled(s, t.ok());
    let warn = |s: String| Span::styled(s, t.warn());
    let err = |s: String| Span::styled(s, t.err());
    let muted = |s: String| Span::styled(s, t.muted());
    let bold = |s: String| Span::styled(s, t.strong());
    let mut l: Vec<Line<'static>> = Vec::new();

    if s.is_root {
        l.push(Line::from(err(" ✗ Запущено від root — запусти obs-legacy від звичайного користувача.".into())));
    }
    if !s.has_pacman {
        l.push(Line::from(warn(" ⚠ pacman не знайдено: OBS-Legacy розрахований на Artix / Arch.".into())));
    }

    l.push(section(t, "Відеокарта і драйвер"));
    if s.gpus.is_empty() {
        l.push(kv(t, "GPU", vec![warn("NVIDIA не виявлено".into())]));
    } else {
        l.push(kv(t, "GPU", vec![bold(s.gpus.join(", "))]));
    }
    let driver = match (s.driver.clone(), s.driver_major()) {
        (Some(v), Some(580)) => vec![bold(v), ok("  ✓ гілка 580xx (legacy)".into())],
        (Some(v), Some(m)) if m > 580 => {
            vec![bold(v), muted("  новіший за 580xx — NVENC працює і в звичайному OBS".into())]
        }
        (Some(v), Some(m)) if m >= 570 => vec![bold(v), ok("  підтримує NVENC API 13.0".into())],
        (Some(v), _) => vec![bold(v), warn("  застарий для NVENC API 13.0".into())],
        (None, _) => vec![err("не знайдено (модуль nvidia не завантажено?)".into())],
    };
    l.push(kv(t, "Драйвер", driver));
    if let Some(p) = &s.driver_pkg {
        l.push(kv(t, "Пакет", vec![muted(p.clone())]));
    }
    let api = match (s.nvenc_api.clone(), s.headers_api.clone(), s.nvenc_compatible()) {
        (Some(a), Some(h), Some(true)) => vec![bold(a), ok(format!("  ✓ сумісно із заголовками збірки ({h})"))],
        (Some(a), Some(h), Some(false)) => {
            vec![bold(a), err(format!("  ✗ замало для заголовків {h} — зміни nv-codec-headers"))]
        }
        (Some(a), _, _) => vec![bold(a)],
        (None, _, _) => vec![warn("невідомо (немає libnvidia-encode.so.1)".into())],
    };
    l.push(kv(t, "NVENC API", api));

    l.push(Line::from(""));
    l.push(section(t, "OBS-Legacy"));
    let active = s.active_build();
    match active {
        Some(b) => {
            l.push(kv(
                t,
                "Активна збірка",
                vec![bold(format!("OBS {}", b.obs_version())), muted(format!("  · {}", b.built()))],
            ));
            let pkgs = match (&s.legacy_pkg, &s.ffmpeg_pkg) {
                _ if b.legacy => vec![warn("стара схема /opt — «Встановити / оновити OBS» поставить пакети".into())],
                (Some(o), Some(f)) => vec![ok(format!("✓ {o}")), muted(format!(" · {f}"))],
                (Some(o), None) => vec![ok(format!("✓ {o}")), muted(" · ffmpeg системний".into())],
                (None, _) => vec![warn("не встановлено".into())],
            };
            l.push(kv(t, "Пакети", pkgs));
            if let Some(p) = b.profile() {
                let mut spans = vec![bold(p)];
                if let Some(cpu) = s.built_for_other_cpu() {
                    spans.push(warn(format!("  ⚠ зібрано під інший процесор ({cpu}) — перезбери")));
                } else if b.get("cpu_tuning") == Some("native") {
                    spans.push(muted("  саме під цей процесор".into()));
                }
                l.push(kv(t, "Оптимізовано", spans));
            }
            match b.get("cef_build") {
                Some(cef) if !cef.is_empty() => {
                    l.push(kv(t, "Browser Source", vec![ok(format!("✓ вбудовано (CEF {cef})"))]))
                }
                _ => l.push(kv(t, "Browser Source", vec![warn("?".into())])),
            }
            if let Some(sdk) = b.get("nvenc_api_headers") {
                let state = match b.get("nvenc_test") {
                    Some("passed") => ok(format!("✓ SDK {sdk}, перевірено obs-nvenc-test")),
                    Some("skipped") => warn(format!("SDK {sdk}, тест NVENC пропущено (SKIP_NVENC_TEST)")),
                    _ => Span::raw(format!("SDK {sdk}")),
                };
                l.push(kv(t, "NVENC", vec![state]));
            }
            let plugins = b.plugins();
            let mut spans: Vec<Span<'static>> = Vec::new();
            if plugins.is_empty() {
                spans.push(muted("немає".into()));
            }
            for (i, (id, ver)) in plugins.iter().enumerate() {
                if i > 0 {
                    spans.push(muted(", ".into()));
                }
                let newer = app
                    .updates
                    .as_ref()
                    .and_then(|u| u.plugins.iter().find(|(pid, _)| pid == id))
                    .and_then(|(_, latest)| latest.clone())
                    .filter(|latest| system::newer(latest, ver));
                spans.push(Span::raw(id.clone()));
                spans.push(muted(format!(" {ver}")));
                if let Some(n) = newer {
                    spans.push(warn(format!(" → {n}")));
                }
            }
            for id in b.plugins_failed() {
                spans.push(err(format!("  ✗ {id} не зібрався")));
            }
            l.push(kv(t, "Плагіни", spans));
        }
        None => l.push(kv(t, "Активна збірка", vec![muted("не встановлено".into())])),
    }
    let obs_cmd = match &s.obs_cmd {
        Some(p) if s.obs_cmd_ok() => vec![ok(format!("✓ {} → OBS-Legacy", p.display()))],
        Some(p) if active.is_some() => vec![warn(format!("{} — не OBS-Legacy (PATH?)", p.display()))],
        Some(p) => vec![muted(format!("{}", p.display()))],
        None => vec![muted("не знайдено".into())],
    };
    l.push(kv(t, "Команда obs", obs_cmd));
    let hook = if s.hook_ours {
        vec![ok("✓ перевіряє бібліотеки після оновлень системи".into())]
    } else if active.is_some() {
        vec![warn("немає".into())]
    } else {
        vec![muted("з'явиться після встановлення".into())]
    };
    l.push(kv(t, "pacman-хук", hook));
    let notifier = match app.notifier {
        None => vec![muted("…".into())],
        Some(Some(b)) => vec![ok(format!("✓ увімкнено ({})", b.label()))],
        Some(None) => vec![muted("вимкнено — «Додатково → Сповіщення про оновлення»".into())],
    };
    l.push(kv(t, "Сповіщення", notifier));
    if s.legacy_leftovers && active.is_some_and(|b| !b.legacy) {
        l.push(kv(
            t,
            "Стара схема",
            vec![warn(
                "лишилися /opt чи обгортка /usr/local/bin/obs — приберуться під час наступного встановлення".into(),
            )],
        ));
    }
    if let Some(p) = &s.obs_pkg {
        let note = if s.legacy_pkg.is_some() {
            " — інший OBS (pacman мав його замінити)"
        } else {
            " — заміниться під час встановлення"
        };
        l.push(kv(t, "Інший OBS", vec![muted(format!("{p}{note}"))]));
    }

    l.push(Line::from(""));
    l.push(section(t, "Система"));
    l.push(kv(t, "ОС", vec![Span::raw(s.os.clone()), muted(format!("  · init: {}", s.init))]));
    l.push(kv(t, "Ядро", vec![Span::raw(s.kernel.clone())]));
    l.push(kv(
        t,
        "Ресурси",
        vec![Span::raw(format!(
            "{} {} · RAM {:.1} ГіБ вільно з {:.1}",
            s.cpus,
            plural(s.cpus, "потік", "потоки", "потоків"),
            s.mem_avail_gib,
            s.mem_total_gib
        ))],
    ));
    let space = |v: Option<f64>, need: f64, what: &str| -> Span<'static> {
        match v {
            Some(g) if g < need => warn(format!("{what} {g:.0} ГіБ (замало, треба ≥{need:.0})")),
            Some(g) => Span::raw(format!("{what} {g:.0} ГіБ")),
            None => muted(format!("{what} ?")),
        }
    };
    l.push(kv(
        t,
        "Вільне місце",
        vec![space(s.free_cache_gib, 10.0, "кеш"), muted(" · ".into()), space(s.free_root_gib, 2.0, "/usr")],
    ));
    l.push(kv(t, "Звіти", vec![muted(truncate(&crate::app::short_path(&reports::dir()), 60))]));
    l
}
