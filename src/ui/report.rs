//! Звіти: список запусків і вбудований переглядач із підсвіткою етапів,
//! попереджень, помилок і порад. Звідси звіт можна відкрити в програмі для
//! тексту, відкрити його теку або скопіювати шлях.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::{buttons, footer, hanging, panel, panel_title, top_bar, truncate, wrap_text};
use crate::analyzer::{Tag, tag};
use crate::app::{App, Target, short_path};
use crate::reports::{self, LineKind, Outcome, ReportInfo};
use crate::theme::Theme;

fn outcome_mark(t: &Theme, r: &ReportInfo) -> (&'static str, Style, String) {
    match r.outcome {
        Outcome::Success if r.warnings > 0 => ("✓", t.ok(), trf!("зауважень: {}", "notes: {}", r.warnings)),
        Outcome::Success => ("✓", t.ok(), tr!("готово", "done").into()),
        Outcome::Failed => ("✗", t.err(), tr!("помилка", "error").into()),
        Outcome::Cancelled => ("■", t.warn(), tr!("скасовано", "cancelled").into()),
        Outcome::Unknown => ("•", t.muted(), tr!("не завершено", "unfinished").into()),
    }
}

pub fn list(f: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme;
    let [bar, _, body, foot] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(5), Constraint::Length(1)])
            .areas(area);
    top_bar(f, app, bar, &[tr!("Звіти", "Reports")], &short_path(&reports::dir()));
    let body = body.inner(ratatui::layout::Margin::new(1, 0));
    let wide = body.width >= 100;
    let (list_r, details_r) = if wide {
        let [l, r] = Layout::horizontal([Constraint::Min(40), Constraint::Length(56)]).spacing(1).areas(body);
        (l, r)
    } else {
        let [l, r] = Layout::vertical([Constraint::Min(5), Constraint::Length(11)]).areas(body);
        (l, r)
    };
    let block = panel(&t, panel_title(&t, trf!("Запуски · {}", "Runs · {}", app.reports.len())), true);
    let inner = block.inner(list_r);
    f.render_widget(block, list_r);

    if app.reports.is_empty() {
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(tr!(" Звітів ще немає.", " No reports yet."), t.strong())),
            Line::from(Span::styled(
                tr!(
                    " Вони з'являються після «Перевірити систему» і «Встановити / оновити OBS».",
                    " They appear after “Check system” and “Install / update OBS”."
                ),
                t.muted(),
            )),
        ])
        .wrap(Wrap { trim: false });
        f.render_widget(msg, inner);
        footer(f, app, foot, &[("Esc", tr!("назад", "back"))], true);
        return;
    }

    app.reports_sel = app.reports_sel.min(app.reports.len() - 1);
    let height = usize::from(inner.height);
    let start = app.reports_sel.saturating_sub(height.saturating_sub(1));
    for (row, i) in (start..app.reports.len()).take(height).enumerate() {
        let r = &app.reports[i];
        let sel = i == app.reports_sel;
        let base = if sel { t.selected() } else { Style::new() };
        let (icon, style, what) = outcome_mark(&t, r);
        let mut spans = vec![
            Span::styled(format!(" {icon} "), if sel { base } else { style }),
            Span::styled(format!("{:<17}", reports::when(&r.stamp)), if sel { base } else { t.muted() }),
            Span::styled(format!("{:<20}", r.label()), if sel { base } else { t.strong() }),
            Span::styled(what, if sel { base } else { style }),
        ];
        let rr = Rect { y: inner.y + row as u16, height: 1, ..inner };
        let pad = usize::from(rr.width).saturating_sub(Line::from(spans.clone()).width());
        spans.push(Span::styled(" ".repeat(pad), base));
        f.render_widget(Paragraph::new(Line::from(spans)), rr);
        app.hits.push((rr, Target::Report(i)));
    }

    // Деталі вибраного звіту
    let r = app.reports[app.reports_sel].clone();
    let (icon, style, what) = outcome_mark(&t, &r);
    let block = panel(&t, panel_title(&t, format!("{} · {}", r.label(), reports::when(&r.stamp))), false);
    let inner = block.inner(details_r).inner(ratatui::layout::Margin::new(1, 0));
    f.render_widget(block, details_r);
    let width = inner.width;
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{icon} "), style.add_modifier(Modifier::BOLD)),
        Span::styled(what, style.add_modifier(Modifier::BOLD)),
    ])];
    if let Some(p) = &r.problem {
        lines.push(Line::from(""));
        let label = if r.outcome == Outcome::Failed {
            tr!("Що сталося  ", "What happened")
        } else {
            tr!("Зверни увагу", "Note         ")
        };
        lines.extend(hanging(Span::styled(format!("{label} "), t.muted()), &p.text, Style::new(), width));
        for h in &p.hints {
            lines.extend(hanging(
                Span::styled(tr!("Що робити    ", "What to do   "), t.muted()),
                h,
                Style::new(),
                width,
            ));
        }
    }
    lines.push(Line::from(""));
    lines.extend(hanging(
        Span::styled(tr!("Файл ", "File "), t.muted()),
        &short_path(&r.path),
        Style::new().fg(t.accent2),
        width,
    ));
    let text_h = inner.height.saturating_sub(2);
    let [text_r, _, buttons_r] =
        Layout::vertical([Constraint::Length(text_h), Constraint::Length(1), Constraint::Length(1)]).areas(inner);
    f.render_widget(Paragraph::new(lines), text_r);
    buttons(
        f,
        app,
        buttons_r,
        &[
            ("Enter", tr!("Переглянути", "View"), true, Target::OpenReport),
            ("c", tr!("Скопіювати шлях", "Copy path"), false, Target::CopyPath),
        ],
        false,
    );

    footer(
        f,
        app,
        foot,
        &[
            ("Esc", tr!("назад", "back")),
            ("Enter", tr!("переглянути", "view")),
            ("c", tr!("копіювати шлях", "copy path")),
            ("e", tr!("відкрити в програмі", "open in an app")),
            ("f", tr!("відкрити теку", "open folder")),
        ],
        true,
    );
}

fn line_style(t: &Theme, kind: LineKind, text: &str) -> Style {
    match kind {
        LineKind::Step => Style::new().fg(t.accent2).add_modifier(Modifier::BOLD),
        LineKind::Ok => t.ok(),
        LineKind::Warn => t.warn(),
        LineKind::Error => t.err().add_modifier(Modifier::BOLD),
        LineKind::Hint => Style::new().fg(t.accent2),
        LineKind::Cancel => t.warn().add_modifier(Modifier::BOLD),
        LineKind::Plain if matches!(tag(text), Some((Tag::Result | Tag::Report, _))) => t.strong(),
        LineKind::Plain => Style::new(),
    }
}

pub fn view(f: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme;
    let [bar, _, body, foot] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(5), Constraint::Length(1)])
            .areas(area);
    let Some(view) = app.report_view.as_ref() else { return };
    let name = view.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let full = short_path(&view.path);
    top_bar(f, app, bar, &[tr!("Звіти", "Reports"), &truncate(&name, 40)], &full);

    let body = body.inner(ratatui::layout::Margin::new(1, 0));
    let Some(view) = app.report_view.as_mut() else { return };
    let block_title = |extra: String| {
        Line::from(vec![Span::styled(tr!(" Звіт ", " Report "), t.title()), Span::styled(extra, t.muted())])
    };
    if let Some(e) = &view.error {
        let block = panel(&t, block_title(String::new()), true);
        let msg = Paragraph::new(vec![
            Line::from(Span::styled(
                trf!(" ✗ Не вдалося відкрити звіт: {e}", " ✗ Couldn't open the report: {e}"),
                t.err(),
            )),
            Line::from(Span::styled(format!("   {}", view.path.display()), t.muted())),
        ])
        .wrap(Wrap { trim: false })
        .block(block);
        f.render_widget(msg, body);
        footer(f, app, foot, &[("Esc", tr!("назад", "back"))], true);
        return;
    }

    // Розмітка: рамка + 1 стовпчик відступу з кожного боку.
    let text_w = body.width.saturating_sub(4);
    if view.wrapped.as_ref().is_none_or(|(w, _)| *w != text_w) {
        let rows: Vec<(usize, String)> = view
            .lines
            .iter()
            .enumerate()
            .flat_map(|(n, l)| wrap_text(l, usize::from(text_w)).into_iter().map(move |row| (n, row)))
            .collect();
        view.wrapped = Some((text_w, rows));
    }
    view.page = usize::from(body.height.saturating_sub(2)).max(1);
    if let Some(target) = view.jump.take()
        && let Some((_, rows)) = &view.wrapped
        && let Some(row) = rows.iter().position(|(n, _)| *n == target)
    {
        view.scroll = row.saturating_sub(view.page / 3);
    }
    view.scroll = view.scroll.min(view.max_scroll());
    let (_, rows) = view.wrapped.as_ref().expect("перенесені рядки щойно пораховано");
    let total = rows.len();
    let first = view.scroll;
    let visible: Vec<Line> = rows
        .iter()
        .skip(first)
        .take(view.page)
        .map(|(n, text)| {
            let line = &view.lines[*n];
            Line::from(Span::styled(text.clone(), line_style(&t, reports::classify(line), line)))
        })
        .collect();
    let pos = if total <= view.page {
        String::from(tr!("· увесь звіт ", "· whole report "))
    } else {
        trf!("· рядки {}–{} з {} ", "· lines {}–{} of {} ", first + 1, (first + view.page).min(total), total)
    };
    let block = panel(&t, block_title(pos), true).padding(ratatui::widgets::Padding::horizontal(1));
    f.render_widget(Paragraph::new(visible).block(block), body);

    footer(
        f,
        app,
        foot,
        &[
            ("Esc", tr!("назад", "back")),
            ("↑↓ PgUp PgDn", tr!("прокрутка", "scroll")),
            ("n/p", tr!("наступна/попередня проблема", "next/previous problem")),
            ("c", tr!("копіювати шлях", "copy path")),
            ("e", tr!("відкрити в програмі", "open in an app")),
        ],
        true,
    );
}
