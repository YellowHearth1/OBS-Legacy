//! Екран виконання: етапи, живий вивід скрипта (вбудований термінал),
//! прогрес компіляції/завантаження, підказки, коли скрипт чекає відповіді, а
//! після завершення — картка результату: що сталося, що робити, кнопки звіту.

use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Gauge, Paragraph};

use super::{buttons, fmt_duration, footer, hanging, panel, spinner, top_bar, truncate};
use crate::analyzer::{Analyzer, Level, Prompt, StepState};
use crate::app::{Action, App, RunState, Target};
use crate::theme::Theme;

const STEPS_W: u16 = 36;

/// Що показати на картці результату.
struct Outcome {
    title: String,
    color: ratatui::style::Color,
    /// (підпис, текст)
    rows: Vec<(&'static str, String)>,
}

/// Підпис головної кнопки картки результату.
fn primary_label(action: Action, wide: bool) -> &'static str {
    match (action, wide) {
        (Action::Deps, true) => "Виправити: встановити пакети",
        (Action::Deps, false) => "Встановити пакети",
        (Action::Clean, true) => "Виправити: очистити кеш",
        (Action::Clean, false) => "Очистити кеш",
        (Action::Check, true) => "Перевірити систему ще раз",
        (Action::Check, false) => "Перевірити ще раз",
        (Action::Rebuild, true) => "Виправити: перезібрати начисто",
        (Action::Rebuild, false) => "Перезібрати",
        (other, _) => crate::app::menu_item(other).label,
    }
}

fn next_step(action: Action) -> Option<&'static str> {
    match action {
        Action::Check => Some("Обери «Встановити / оновити OBS»: він збере й встановить OBS з NVENC."),
        Action::Deps => Some("Перевір систему ще раз (Enter) — тепер усі етапи мають бути ✓."),
        Action::Install | Action::Rebuild => {
            Some("Запускай OBS як завжди — з меню програм, командою obs або пунктом «Запустити OBS».")
        }
        Action::Rollback => Some("OBS уже перемкнено. Якщо OBS зараз відкритий — перезапусти його."),
        Action::SelfUpdate => Some("Закрий і знову відкрий OBS-Legacy, щоб запрацювала нова версія."),
        _ => None,
    }
}

fn outcome(t: &Theme, run: &RunState) -> Option<Outcome> {
    let exit = run.job.exit?;
    let a: &Analyzer = &run.job.analyzer;
    let elapsed = fmt_duration(run.job.elapsed());
    if let Some(msg) = &a.cancelled {
        return Some(Outcome {
            title: format!("■ {} — скасовано", run.title),
            color: t.warn,
            rows: vec![("Що сталося", msg.clone())],
        });
    }
    if run.interrupted {
        return Some(Outcome {
            title: format!("■ {} — перервано (Ctrl+C)", run.title),
            color: t.warn,
            rows: vec![(
                "Що сталося",
                "Завдання зупинено. Поточний OBS не змінювався, поки нову збірку не перевірено.".into(),
            )],
        });
    }
    if exit.success {
        let mut rows = Vec::new();
        // Що саме змінилося («OBS 33.0.0 → 33.0.1», «оновлень немає») — першим рядком.
        if let Some(sum) = &a.summary {
            rows.push(("Підсумок", sum.clone()));
        }
        if let Some(p) = a.last_notice().filter(|p| p.level == Level::Warning && !p.hints.is_empty()) {
            rows.push(("Зверни увагу", p.text.clone()));
            rows.extend(p.hints.iter().map(|h| ("Що далі", h.clone())));
        } else if let Some(step) = run.next.as_deref().or(next_step(run.action)) {
            rows.push(("Що далі", step.to_string()));
        }
        return Some(Outcome {
            title: format!("✓ {} — готово за {elapsed}", run.title), color: t.ok, rows
        });
    }
    let mut rows = Vec::new();
    match a.last_error() {
        Some(p) => {
            rows.push(("Що сталося", p.text.clone()));
            rows.extend(p.hints.iter().map(|h| ("Що робити", h.clone())));
            if p.hints.is_empty() {
                rows.push((
                    "Що робити",
                    "Переглянь звіт (o): там повний вивід. Якщо незрозуміло, що робити, створи issue на GitHub \
                     і додай звіт."
                        .into(),
                ));
            }
        }
        None => rows.push(("Що сталося", format!("Скрипт завершився з кодом {}.", exit.code))),
    }
    Some(Outcome { title: format!("✗ {} — помилка", run.title), color: t.err, rows })
}

/// Рядки картки результату для заданої ширини тексту.
fn outcome_lines(t: &Theme, o: &Outcome, width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (label, text) in &o.rows {
        let marker = Span::styled(format!("{label:<13}"), t.muted());
        lines.extend(hanging(marker, text, Style::new(), width));
    }
    lines
}

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme;
    let tick = app.tick;
    let now = Instant::now();
    let primary = app.run_primary();
    let Some(run) = app.run.as_mut() else { return };
    let running = run.job.running();
    let prompt = if running { run.job.analyzer.prompt(now) } else { None };
    let progress_h = u16::from(running && (run.job.analyzer.compile.is_some() || run.job.analyzer.download.is_some()));
    let banner_h = u16::from(prompt.is_some());
    let result = outcome(&t, run);
    let result_lines = result.as_ref().map(|o| outcome_lines(&t, o, area.width.saturating_sub(6))).unwrap_or_default();
    // Рамка + текст (до 8 рядків) + порожній рядок + кнопки.
    let result_h = if result.is_some() { (result_lines.len() as u16).min(8) + 4 } else { 0 };

    let [bar, body, progress, banner, result_r, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(progress_h),
        Constraint::Length(banner_h),
        Constraint::Length(result_h),
        Constraint::Length(1),
    ])
    .areas(area);

    // Верхня смуга: назва, стан, час, лічильники.
    let a = &run.job.analyzer;
    let cancelled = a.cancelled.is_some() || run.interrupted;
    let status = match run.job.exit {
        None => format!("{} {}", spinner(tick), fmt_duration(run.job.elapsed())),
        Some(e) if e.success => format!("✓ {}", fmt_duration(run.job.elapsed())),
        Some(_) if cancelled => format!("■ {}", fmt_duration(run.job.elapsed())),
        Some(_) => format!("✗ {}", fmt_duration(run.job.elapsed())),
    };
    let counts = format!("⚠ {}  ✗ {}  {status}", a.warnings.len(), a.errors.len());
    let title = run.title.clone();
    // top_bar потребує &App, тож спершу копіюємо з run усе потрібне.
    let step_title = a.steps.last().filter(|_| running).map(|s| s.title.clone());
    // Знак (~16) + « › назва» + « › етап» + лічильники праворуч: етап отримує, що лишилося.
    let room = usize::from(bar.width).saturating_sub(16 + 5 + title.chars().count() + 5 + counts.chars().count() + 5);
    let mut crumbs = vec![title.clone()];
    if let Some(step) = step_title.filter(|_| room >= 12) {
        crumbs.push(truncate(&step, room));
    }
    let crumb_refs: Vec<&str> = crumbs.iter().map(String::as_str).collect();
    top_bar(f, app, bar, &crumb_refs, &counts);

    let Some(run) = app.run.as_mut() else { return };
    let a = &run.job.analyzer;

    // Етапи + вивід
    let body = body.inner(ratatui::layout::Margin::new(1, 0));
    let show_steps = body.width >= 100;
    let (steps_r, out_r) = if show_steps {
        let [s, o] = Layout::horizontal([Constraint::Length(STEPS_W), Constraint::Min(20)]).spacing(1).areas(body);
        (Some(s), o)
    } else {
        (None, body)
    };
    if let Some(sr) = steps_r {
        let block = panel(&t, Line::from(Span::styled(" Етапи ", t.title())), false);
        let inner = block.inner(sr);
        f.render_widget(block, sr);
        let visible = usize::from(inner.height);
        let skip = a.steps.len().saturating_sub(visible);
        let lines: Vec<Line> = a
            .steps
            .iter()
            .skip(skip)
            .map(|s| {
                let (icon, style) = match s.state {
                    StepState::Running => (spinner(tick), Style::new().fg(t.accent2).add_modifier(Modifier::BOLD)),
                    StepState::Ok => ("✓", t.ok()),
                    StepState::Warn => ("⚠", t.warn()),
                    StepState::Failed => ("✗", t.err()),
                };
                let time = fmt_duration(s.elapsed(now));
                let room = usize::from(inner.width).saturating_sub(time.chars().count() + 4);
                let title = truncate(&s.title, room);
                let pad = room.saturating_sub(title.chars().count());
                let title_style = if s.state == StepState::Running { t.strong() } else { Style::new() };
                Line::from(vec![
                    Span::styled(format!(" {icon} "), style),
                    Span::styled(title, title_style),
                    Span::raw(" ".repeat(pad + 1)),
                    Span::styled(time, t.muted()),
                ])
            })
            .collect();
        f.render_widget(Paragraph::new(lines), inner);
    }

    let scrolled = run.scroll > 0;
    let mut title = vec![Span::styled(" Вивід ", t.title())];
    if scrolled {
        title.push(Span::styled(format!("↑ прокручено на {} · End — до кінця ", run.scroll), t.warn()));
    } else if running {
        title.push(Span::styled("(відповідай на запитання прямо тут) ", t.muted()));
    }
    let block = panel(&t, Line::from(title), running && prompt.is_some());
    let inner = block.inner(out_r);
    f.render_widget(block, out_r);
    run.output_area = inner;
    run.job.resize(inner.height, inner.width);
    run.job.parser.screen_mut().set_scrollback(run.scroll);
    run.scroll = run.job.parser.screen().scrollback();
    let screen = run.job.parser.screen();
    f.render_widget(crate::term_view::TermView::new(screen, &t), inner);
    if running && run.scroll == 0 && !screen.hide_cursor() {
        let (row, col) = screen.cursor_position();
        if row < inner.height && col < inner.width {
            f.set_cursor_position((inner.x + col, inner.y + row));
        }
    }

    // Прогрес
    if progress_h > 0 {
        let a = &run.job.analyzer;
        let (ratio, label) = if let Some(p) = a.compile {
            let what = a.steps.last().map_or("Компіляція", |s| {
                if s.title.starts_with("Плагін") { "Плагін" } else { "Компіляція" }
            });
            let eta = p.eta().map(|d| format!(" · залишилось ~{}", fmt_duration(d))).unwrap_or_default();
            (p.ratio(), format!("{what} {}/{} · {:.0}%{eta}", p.done, p.total, p.ratio() * 100.0))
        } else {
            let pct = a.download.unwrap_or(0.0);
            (pct / 100.0, format!("Завантаження {pct:.1}%"))
        };
        let gauge = Gauge::default()
            .ratio(ratio.clamp(0.0, 1.0))
            .label(Span::styled(label, t.strong()))
            .gauge_style(Style::new().fg(t.accent).bg(t.raised))
            .use_unicode(true);
        f.render_widget(gauge, progress.inner(ratatui::layout::Margin::new(1, 0)));
    }

    // Скрипт чекає відповіді
    if let Some(p) = prompt {
        let text = match p {
            Prompt::Confirm => " ⌨  Скрипт питає: введи y (так) або n (ні) і натисни Enter ",
            Prompt::Password => " ⌨  Введи пароль sudo/doas і натисни Enter (символів не видно — так і має бути) ",
            Prompt::Input => " ⌨  Схоже, скрипт чекає на відповідь — введи її у вікні виводу ",
        };
        f.render_widget(Paragraph::new(Span::styled(text, t.banner(t.warn))), banner);
    }

    // Картка результату
    let has_report = run.job.analyzer.report.is_some();
    if let Some(o) = &result {
        let r = result_r.inner(ratatui::layout::Margin::new(1, 0));
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(o.color))
            .style(t.panel())
            .title(Line::from(Span::styled(
                format!(" {} ", o.title),
                Style::new().fg(o.color).add_modifier(Modifier::BOLD),
            )));
        let inner = block.inner(r).inner(ratatui::layout::Margin::new(1, 0));
        f.render_widget(block, r);
        let text_h = inner.height.saturating_sub(2);
        let [text_r, _, buttons_r] =
            Layout::vertical([Constraint::Length(text_h), Constraint::Length(1), Constraint::Length(1)]).areas(inner);
        let lines = outcome_lines(&t, o, inner.width);
        f.render_widget(Paragraph::new(lines), text_r);
        // У вузькому вікні (або поруч із кнопкою виправлення) — короткі підписи, щоб влізли всі кнопки.
        let wide = buttons_r.width >= 96;
        let roomy = wide && primary.is_none();
        let mut items: Vec<(&str, &str, bool, Target)> = Vec::new();
        // Головна кнопка — виправлення, яке запропонував скрипт (або повторна перевірка).
        if let Some(action) = primary {
            items.push(("Enter", primary_label(action, wide), true, Target::RunPrimary));
        }
        if has_report {
            items.push(("o", if roomy { "Переглянути звіт" } else { "Звіт" }, false, Target::OpenReport));
            items.push(("c", if roomy { "Скопіювати шлях" } else { "Шлях" }, false, Target::CopyPath));
            items.push(("e", if roomy { "Відкрити в програмі" } else { "У програмі" }, false, Target::OpenExternal));
        }
        if primary.is_some() {
            items.push(("Esc", "До меню", false, Target::Continue));
        } else {
            items.push(("Enter", "До меню", true, Target::Continue));
        }
        buttons(f, app, buttons_r, &items, false);
    }

    let hints: Vec<(&str, &str)> = if running {
        vec![("↑↓ PgUp/PgDn", "прокрутка"), ("Ctrl+C ×2", "перервати"), ("", "літери й Enter ідуть у скрипт")]
    } else if let Some(action) = primary {
        let what = if action == Action::Check { "перевірити ще раз" } else { "виправити" };
        vec![("Enter", what), ("Esc", "до меню"), ("o", "звіт"), ("c", "копіювати шлях"), ("PgUp/PgDn", "прокрутка")]
    } else if has_report {
        vec![("Enter", "до меню"), ("o", "звіт"), ("c", "копіювати шлях"), ("PgUp/PgDn", "прокрутка")]
    } else {
        vec![("Enter", "до меню"), ("PgUp/PgDn", "прокрутка")]
    };
    footer(f, app, foot, &hints, false);
}
