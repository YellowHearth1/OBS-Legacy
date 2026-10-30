//! Екрани-списки: плагіни (галочки), відкат на збережену збірку, налаштування.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::{footer, kv, panel, panel_title, spinner, top_bar};
use crate::app::{App, Target, short_path};
use crate::settings::{self, Kind, Settings};
use crate::theme::Theme;

/// Верхня смуга, порожній рядок, тіло (з полями 1 стовпчик), нижня смуга.
fn frame(area: Rect) -> (Rect, Rect, Rect) {
    let [bar, _, body, foot] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(5), Constraint::Length(1)])
            .areas(area);
    (bar, body.inner(ratatui::layout::Margin::new(1, 0)), foot)
}

/// Список ліворуч і деталі праворуч (або знизу у вузькому вікні).
fn split(body: Rect, details_w: u16) -> (Rect, Rect) {
    if body.width >= 100 {
        let [l, r] = Layout::horizontal([Constraint::Min(40), Constraint::Length(details_w)]).spacing(1).areas(body);
        (l, r)
    } else {
        let [l, r] = Layout::vertical([Constraint::Min(6), Constraint::Length(10)]).areas(body);
        (l, r)
    }
}

/// Рядки списку з прокруткою, що тримає вибраний рядок у полі зору.
fn visible_range(selected: usize, len: usize, height: usize) -> std::ops::Range<usize> {
    if len <= height {
        return 0..len;
    }
    let start = selected.saturating_sub(height / 2).min(len - height);
    start..start + height
}

fn row_line<'a>(t: &Theme, selected: bool, spans: Vec<Span<'a>>, width: u16) -> Line<'a> {
    let style = if selected { t.selected() } else { Style::new() };
    let mut line = Line::from(
        spans.into_iter().map(|s| if selected { Span::styled(s.content, style) } else { s }).collect::<Vec<_>>(),
    );
    let pad = width.saturating_sub(line.width() as u16);
    line.push_span(Span::styled(" ".repeat(usize::from(pad)), style));
    line
}

fn details_panel<'a>(t: &Theme, title: &str, lines: Vec<Line<'a>>) -> Paragraph<'a> {
    Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .block(panel(t, panel_title(t, title), false).padding(ratatui::widgets::Padding::horizontal(1)))
}

pub fn plugins(f: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme;
    let (bar, body, foot) = frame(area);
    top_bar(f, app, bar, &["Плагіни"], "збираються разом з OBS і оновлюються разом з ним");
    let (list_r, details_r) = split(body, 52);
    let selected_ids = app.settings.plugins();
    let installed: Vec<(String, String)> =
        app.sys.as_ref().and_then(|s| s.active_build()).map(|b| b.plugins()).unwrap_or_default();

    let block = panel(
        &t,
        Line::from(vec![
            Span::styled(" Каталог ", t.title()),
            Span::styled(format!("· обрано {} ", selected_ids.len()), t.muted()),
        ]),
        true,
    );
    let inner = block.inner(list_r);
    f.render_widget(block, list_r);

    let Some(catalog) = app.catalog.clone() else {
        let msg = match &app.catalog_error {
            Some(e) => Line::from(Span::styled(format!(" ✗ Не вдалося прочитати каталог: {e}"), t.err())),
            None => Line::from(format!(" {} Завантажую каталог плагінів…", spinner(app.tick))),
        };
        f.render_widget(Paragraph::new(msg).wrap(Wrap { trim: true }), inner);
        footer(f, app, foot, &[("Esc", "назад")], true);
        return;
    };
    app.plugin_sel = app.plugin_sel.min(catalog.len().saturating_sub(1));
    let name_w = catalog.iter().map(|p| p.name.chars().count()).max().unwrap_or(10);
    for (row, i) in visible_range(app.plugin_sel, catalog.len(), usize::from(inner.height)).enumerate() {
        let p = &catalog[i];
        let checked = selected_ids.contains(&p.id);
        let sel = i == app.plugin_sel;
        let mut spans = vec![
            Span::styled(
                if checked { " [✓] " } else { " [ ] " },
                if checked { t.ok().add_modifier(Modifier::BOLD) } else { t.muted() },
            ),
            Span::styled(format!("{:<name_w$}", p.name), if checked { t.strong() } else { Style::new() }),
        ];
        if p.default {
            spans.push(Span::styled("  ◆ радимо", t.warn()));
        }
        if let Some((_, v)) = installed.iter().find(|(id, _)| *id == p.id) {
            spans.push(Span::styled(format!("  ● встановлено {v}"), t.ok()));
        }
        let r = Rect { y: inner.y + row as u16, height: 1, ..inner };
        f.render_widget(Paragraph::new(row_line(&t, sel, spans, r.width)), r);
        app.hits.push((r, Target::Plugin(i)));
    }

    if let Some(p) = catalog.get(app.plugin_sel) {
        let mut lines = vec![
            Line::from(Span::styled(p.short_repo().to_string(), Style::new().fg(t.accent2))),
            Line::from(""),
            Line::from(p.description.clone()),
            Line::from(""),
        ];
        if p.default {
            lines.push(Line::from(Span::styled("◆ Радимо: увімкнено за замовчуванням.", t.warn())));
        }
        lines.push(kv(&t, "id", vec![Span::styled(p.id.clone(), t.muted())]));
        lines.push(kv(&t, "Пакет", vec![Span::raw(p.package.clone())]));
        if !p.packages.is_empty() {
            lines.push(kv(&t, "Потребує", vec![Span::raw(p.packages.clone())]));
        }
        let latest = app
            .updates
            .as_ref()
            .and_then(|u| u.plugins.iter().find(|(id, _)| *id == p.id))
            .and_then(|(_, v)| v.clone());
        match installed.iter().find(|(id, _)| *id == p.id) {
            Some((_, v)) => lines.push(kv(&t, "Встановлено", vec![Span::styled(v.clone(), t.ok())])),
            None => lines.push(kv(&t, "Встановлено", vec![Span::styled("ні", t.muted())])),
        }
        if let Some(l) = latest {
            lines.push(kv(&t, "Найновіший", vec![Span::raw(l)]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Галочка зберігається одразу, а плагін з'явиться після «Встановити / оновити OBS»: він збирається \
             з вихідних кодів під твій процесор і ставиться окремим пакетом. Якщо він колись не збереться, OBS \
             встановиться без нього, з попередженням.",
            t.muted(),
        )));
        f.render_widget(details_panel(&t, &p.name, lines), details_r);
    }
    footer(
        f,
        app,
        foot,
        &[("Esc", "назад"), ("Пробіл", "галочка"), ("a", "усі"), ("n", "жодного"), ("d", "типові")],
        true,
    );
}

pub fn rollback(f: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme;
    let (bar, body, foot) = frame(area);
    top_bar(f, app, bar, &["Відкат на іншу збірку"], "попередній набір пакетів — без перезбирання");
    let (list_r, details_r) = split(body, 52);
    let block = panel(&t, panel_title(&t, "Збережені збірки"), true);
    let inner = block.inner(list_r);
    f.render_widget(block, list_r);
    if app.builds.is_empty() {
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(" Збережених збірок ще немає.", t.strong())),
            Line::from(Span::styled(
                " Після першого «Встановити / оновити OBS» тут з'являться набори пакетів для відкату.",
                t.muted(),
            )),
        ])
        .wrap(Wrap { trim: false });
        f.render_widget(msg, inner);
        let about = details_panel(
            &t,
            "Як працює відкат",
            vec![
                Line::from(
                    "Кожне збирання дає набір пакетів pacman (OBS, плагіни, за потреби ffmpeg), і набори \
                     зберігаються. Відкат ставить попередній набір через pacman -U — без перезбирання.",
                ),
                Line::from(""),
                Line::from(Span::styled(
                    "Скільки наборів зберігати, задає «Налаштування → Скільки наборів тримати».",
                    t.muted(),
                )),
            ],
        );
        f.render_widget(about, details_r);
    } else {
        app.build_sel = app.build_sel.min(app.builds.len() - 1);
        for (row, i) in visible_range(app.build_sel, app.builds.len(), usize::from(inner.height)).enumerate() {
            let b = &app.builds[i];
            let sel = i == app.build_sel;
            let mut spans = vec![
                Span::styled(if b.active { " ● " } else { "   " }, t.ok()),
                Span::styled(format!("OBS {:<10}", b.obs_version()), t.strong()),
                Span::styled(format!(" {}  ", b.built()), t.muted()),
            ];
            if b.active {
                spans.push(Span::styled("активна ", t.ok()));
            }
            if b.legacy {
                spans.push(Span::styled("/opt", t.muted()));
            }
            if !b.complete {
                spans.push(Span::styled("неповна", t.err()));
            }
            let r = Rect { y: inner.y + row as u16, height: 1, ..inner };
            f.render_widget(Paragraph::new(row_line(&t, sel, spans, r.width)), r);
            app.hits.push((r, Target::Build(i)));
        }
        if let Some(b) = app.builds.get(app.build_sel) {
            let get = |k: &str| b.get(k).unwrap_or("—").to_string();
            let mut lines = vec![
                Line::from(Span::styled(b.label.clone(), t.muted())),
                Line::from(""),
                kv(&t, "OBS", vec![Span::raw(get("obs_version"))]),
                kv(&t, "Коміт", vec![Span::raw(get("obs_commit").chars().take(12).collect::<String>())]),
                kv(&t, "Зібрано", vec![Span::raw(b.built())]),
                kv(&t, "CEF", vec![Span::raw(get("cef_build"))]),
                kv(&t, "NVENC SDK", vec![Span::raw(get("nvenc_api_headers"))]),
            ];
            if let Some(p) = b.profile() {
                lines.push(kv(&t, "Оптимізовано", vec![Span::raw(p)]));
            }
            let plugins = b.plugins();
            let text = if plugins.is_empty() {
                "немає".to_string()
            } else {
                plugins.iter().map(|(id, v)| format!("{id} {v}")).collect::<Vec<_>>().join(", ")
            };
            lines.push(kv(&t, "Плагіни", vec![Span::raw(text)]));
            if let Some(v) = b.get("ffmpeg") {
                lines.push(kv(&t, "ffmpeg", vec![Span::raw(format!("{v} (у наборі)"))]));
            }
            if b.legacy {
                lines.push(kv(&t, "Схема", vec![Span::styled("стара (/opt, до пакетів)", t.muted())]));
            }
            lines.push(Line::from(""));
            let hint = if b.active {
                Span::styled("Ця збірка зараз активна.", t.ok())
            } else if b.complete && b.legacy {
                Span::styled(
                    "Enter — перемкнути OBS на цю збірку в /opt (скрипт попросить підтвердження і пароль).",
                    t.muted(),
                )
            } else if b.complete {
                Span::styled(
                    "Enter — поставити цей набір пакетів через pacman (скрипт попросить підтвердження і пароль).",
                    t.muted(),
                )
            } else {
                Span::styled("Збірка неповна — перемкнутися на неї не можна.", t.err())
            };
            lines.push(Line::from(hint));
            f.render_widget(details_panel(&t, &format!("OBS {}", b.obs_version()), lines), details_r);
        }
    }
    footer(f, app, foot, &[("Esc", "назад"), ("↑↓", "вибір"), ("Enter", "перемкнутися")], true);
}

fn value_spans(t: &Theme, settings: &Settings, key: &str, kind: Kind) -> Vec<Span<'static>> {
    let v = settings.get(key);
    let mut spans = match kind {
        Kind::Toggle if v == "1" => vec![Span::styled("[✓] увімкнено", t.ok())],
        Kind::Toggle => vec![Span::styled("[ ] вимкнено", t.muted())],
        Kind::Number { .. } if v.is_empty() => vec![Span::styled("авто", t.muted())],
        Kind::Choice(_) => match kind.choice_label(&v) {
            Some(label) => vec![Span::raw(v.clone()), Span::styled(format!(" ({label})"), t.muted())],
            None => vec![Span::styled(v.clone(), t.warn())],
        },
        _ if v.is_empty() => vec![Span::styled("—", t.muted())],
        _ if key == "OBS_REF" && v == "stable" => {
            vec![Span::raw("stable"), Span::styled(" (найновіша стабільна)", t.muted())]
        }
        _ => vec![Span::raw(v)],
    };
    if let Some(env) = Settings::env_override(key) {
        spans.push(Span::styled(format!("  (змінна середовища: {env})"), t.warn()));
    }
    spans
}

pub fn settings(f: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme;
    let (bar, body, foot) = frame(area);
    let path = short_path(app.settings.path());
    top_bar(f, app, bar, &["Налаштування"], &format!("{path} · діють і для скрипта без TUI"));
    let (list_r, details_r) = split(body, 50);
    let block = panel(&t, panel_title(&t, "Параметри"), true);
    let inner = block.inner(list_r);
    f.render_widget(block, list_r);

    // Рядки списку: заголовки груп (None) і поля (Some(індекс)).
    let mut rows: Vec<Option<usize>> = vec![None];
    for (i, field) in settings::FIELDS.iter().enumerate() {
        if field.advanced && (i == 0 || !settings::FIELDS[i - 1].advanced) {
            rows.push(None);
        }
        rows.push(Some(i));
    }
    let sel_row = rows.iter().position(|r| *r == Some(app.settings_sel)).unwrap_or(0);
    let label_w = settings::FIELDS.iter().map(|f| f.label.chars().count()).max().unwrap_or(20) + 2;
    let mut seen_header = 0;
    let range = visible_range(sel_row, rows.len(), usize::from(inner.height));
    // Номер заголовка, з якого починається видима частина.
    seen_header += rows[..range.start].iter().filter(|r| r.is_none()).count();
    for (row, ri) in range.enumerate() {
        let r = Rect { y: inner.y + row as u16, height: 1, ..inner };
        let Some(i) = rows[ri] else {
            let text = if seen_header == 0 { " Основне" } else { " Для досвідчених" };
            seen_header += 1;
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(text, Style::new().fg(t.accent2).add_modifier(Modifier::BOLD)))),
                r,
            );
            continue;
        };
        let field = &settings::FIELDS[i];
        let sel = i == app.settings_sel;
        let label = Span::styled(format!("   {:<label_w$}", field.label), Style::new().add_modifier(Modifier::BOLD));
        if sel && let Some(ed) = &app.editor {
            let text = ed.text();
            let line =
                Line::from(vec![label, Span::styled(format!("{text} "), Style::new().fg(t.on_accent).bg(t.accent2))]);
            f.render_widget(Paragraph::new(line), r);
            let prefix: String = ed.chars[..ed.cursor].iter().collect();
            let x = r.x + 3 + label_w as u16 + Line::from(prefix).width() as u16;
            if x < r.right() {
                f.set_cursor_position((x, r.y));
            }
        } else {
            let mut spans = vec![label];
            spans.extend(value_spans(&t, &app.settings, field.key, field.kind));
            f.render_widget(Paragraph::new(row_line(&t, sel, spans, r.width)), r);
        }
        app.hits.push((r, Target::Setting(i)));
    }

    let field = &settings::FIELDS[app.settings_sel];
    let mut lines = vec![
        Line::from(Span::styled(field.key, Style::new().fg(t.accent2))),
        Line::from(""),
        Line::from(field.help),
        Line::from(""),
        kv(
            &t,
            "Типово",
            vec![Span::raw(match (field.kind, field.default) {
                (Kind::Toggle, "1") => "увімкнено".to_string(),
                (Kind::Toggle, _) => "вимкнено".to_string(),
                (Kind::Number { .. }, "") => "авто".to_string(),
                (k @ Kind::Choice(_), d) => format!("{d} ({})", k.choice_label(d).unwrap_or("")),
                (_, "") => "—".to_string(),
                (_, d) => d.to_string(),
            })],
        ),
    ];
    if let Some(e) = &app.settings_error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(format!("✗ {e}"), t.err())));
    }
    f.render_widget(details_panel(&t, field.label, lines), details_r);

    let hints: &[(&str, &str)] = if app.editor.is_some() {
        &[("Enter", "зберегти"), ("Esc", "скасувати"), ("Ctrl+U", "очистити")]
    } else {
        &[("Esc", "назад"), ("Enter", "змінити"), ("r", "типове значення")]
    };
    footer(f, app, foot, hints, app.editor.is_none());
}

#[cfg(test)]
mod tests {
    use super::visible_range;

    #[test]
    fn keeps_selection_visible() {
        assert_eq!(visible_range(0, 5, 10), 0..5);
        assert_eq!(visible_range(0, 30, 10), 0..10);
        assert_eq!(visible_range(15, 30, 10), 10..20);
        assert_eq!(visible_range(29, 30, 10), 20..30);
    }
}
