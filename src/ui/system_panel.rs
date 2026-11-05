//! Панель «Твоя система» на головному екрані: коротко — по рядку на те, що важливо новачкові, докладно
//! (клавіша d) — усе, що відомо про систему й збірки.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::{kv, lower_first, panel, section, spinner, truncate, wrap_spans};
use crate::app::{App, Target};
use crate::i18n::count_word;
use crate::reports::{self, Outcome};
use crate::theme::Theme;
use crate::versions;

pub(super) fn draw_system(f: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme;
    let toggle = if app.details {
        tr!(" d — коротко ", " d — brief ")
    } else {
        tr!(" d — подробиці ", " d — details ")
    };
    let mut title = vec![Span::styled(tr!(" Твоя система ", " Your system "), t.title())];
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
            Span::raw(tr!("Збираю відомості про систему…", "Gathering system information…")),
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
        Some(g) => l.extend(ok(tr!("Відеокарта", "Graphics card"), g.trim_start_matches("NVIDIA ").to_string())),
        None => l.extend(row(
            "✗",
            t.err(),
            tr!("Відеокарта", "Graphics card"),
            vec![Span::styled(tr!("NVIDIA не знайдено", "no NVIDIA found"), t.err())],
        )),
    }
    match (s.driver.clone(), s.driver_major()) {
        (Some(v), Some(580)) => l.extend(ok(tr!("Драйвер", "Driver"), trf!("{v} · гілка 580xx", "{v} · 580xx branch"))),
        (Some(v), Some(m)) if m > 580 => l.extend(row(
            "•",
            t.muted(),
            tr!("Драйвер", "Driver"),
            vec![Span::raw(v), Span::styled(tr!(" · новіший за 580xx", " · newer than 580xx"), t.muted())],
        )),
        (Some(v), _) => l.extend(ok(tr!("Драйвер", "Driver"), v)),
        (None, _) => l.extend(row(
            "✗",
            t.err(),
            tr!("Драйвер", "Driver"),
            vec![Span::styled(tr!("не завантажено", "not loaded"), t.err())],
        )),
    }
    match (s.nvenc_api.clone(), s.nvenc_compatible()) {
        (Some(a), Some(true) | None) => l.extend(ok("NVENC", trf!("API {a} — підходить", "API {a} — fine"))),
        (Some(a), Some(false)) => l.extend(row(
            "✗",
            t.err(),
            "NVENC",
            vec![Span::styled(trf!("API {a} — замало", "API {a} — too old"), t.err())],
        )),
        (None, _) => l.extend(row("⚠", t.warn(), "NVENC", vec![Span::styled(tr!("невідомо", "unknown"), t.warn())])),
    }
    let free = s.free_cache_gib.into_iter().chain(s.free_root_gib).fold(f64::INFINITY, f64::min);
    if free.is_finite() {
        if free < 10.0 {
            l.extend(row(
                "⚠",
                t.warn(),
                tr!("Диск", "Disk"),
                vec![Span::styled(
                    trf!("вільно {free:.0} ГіБ — треба ≥ 10", "{free:.0} GiB free — needs ≥ 10"),
                    t.warn(),
                )],
            ));
        } else {
            l.extend(ok(tr!("Диск", "Disk"), trf!("вільно {free:.0} ГіБ", "{free:.0} GiB free")));
        }
    }

    match app.missing_packages.as_deref() {
        Some([]) if app.missing_norepo.is_empty() => {
            l.extend(ok(tr!("Пакети", "Packages"), tr!("усі на місці", "all in place").into()))
        }
        Some(m) if !m.is_empty() => l.extend(row(
            "⚠",
            t.warn(),
            tr!("Пакети", "Packages"),
            vec![Span::styled(trf!("бракує {}: {}", "{} missing: {}", m.len(), m.join(", ")), t.warn())],
        )),
        Some(_) => l.extend(row(
            "✗",
            t.err(),
            tr!("Пакети", "Packages"),
            vec![Span::styled(
                trf!("немає в репозиторіях: {}", "not in the repositories: {}", app.missing_norepo.join(", ")),
                t.err(),
            )],
        )),
        None => {}
    }

    let active = s.active_build();
    match active {
        Some(b) => {
            l.extend(ok(
                tr!("OBS з NVENC", "OBS with NVENC"),
                trf!("OBS {} · зібрано {}", "OBS {} · built {}", b.obs_version(), b.built()),
            ));
            if b.get("cef_build").is_none_or(str::is_empty) {
                l.extend(row(
                    "⚠",
                    t.warn(),
                    "Browser Source",
                    vec![Span::styled(tr!("невідомо", "unknown"), t.warn())],
                ));
            }
            match (b.legacy, b.profile()) {
                (true, _) => l.extend(row(
                    "⚠",
                    t.warn(),
                    tr!("Схема", "Layout"),
                    vec![Span::styled(
                        tr!(
                            "стара (/opt) — онови, щоб отримати пакети pacman",
                            "old (/opt) — update to get pacman packages"
                        ),
                        t.warn(),
                    )],
                )),
                (false, Some(p)) => l.extend(ok(tr!("Оптимізовано", "Tuned for"), p)),
                (false, None) => {}
            }
            if s.obs_cmd_ok() {
                l.extend(ok(
                    tr!("Команда obs", "obs command"),
                    tr!("відкриває OBS з NVENC", "opens OBS with NVENC").into(),
                ));
            } else {
                l.extend(row(
                    "⚠",
                    t.warn(),
                    tr!("Команда obs", "obs command"),
                    vec![Span::styled(
                        tr!("веде не на OBS-Legacy (PATH?)", "doesn't lead to OBS-Legacy (PATH?)"),
                        t.warn(),
                    )],
                ));
            }
        }
        None => l.extend(row(
            "•",
            t.muted(),
            tr!("OBS з NVENC", "OBS with NVENC"),
            vec![Span::styled(tr!("ще не встановлено", "not installed yet"), t.muted())],
        )),
    }

    let selected = app.settings.plugins();
    let installed = active.map(|b| b.plugins()).unwrap_or_default();
    let mut plugins: Vec<Span<'static>> = Vec::new();
    if selected.is_empty() && installed.is_empty() {
        plugins.push(Span::styled(tr!("без плагінів", "no plugins"), t.muted()));
    }
    for (i, id) in selected.iter().enumerate() {
        if i > 0 {
            plugins.push(Span::styled(", ", t.muted()));
        }
        plugins.push(Span::raw(id.clone()));
        match installed.iter().find(|(pid, _)| pid == id) {
            Some((_, v)) => plugins.push(Span::styled(format!(" {v}"), t.muted())),
            None if active.is_some() => plugins.push(Span::styled(tr!(" (буде додано)", " (will be added)"), t.warn())),
            None => {}
        }
    }
    let plugins_icon = if active.is_some() { ("✓", t.ok()) } else { ("•", t.muted()) };
    l.extend(row(plugins_icon.0, plugins_icon.1, tr!("Плагіни", "Plugins"), plugins));

    if app.updates_loading {
        l.extend(row(
            spinner(app.tick),
            Style::new().fg(t.accent2),
            tr!("Оновлення", "Updates"),
            vec![Span::styled(tr!("перевіряю…", "checking…"), t.muted())],
        ));
    } else if let Some(f) = app.facts()
        && let Some(latest) = f.obs_latest.clone()
    {
        let news = if let Some(l) = f.obs_update() {
            Some(trf!("вийшов OBS {l}", "OBS {l} is out"))
        } else if !f.plugin_updates.is_empty() {
            Some(trf!("нові версії плагінів: {}", "new plugin versions: {}", f.plugin_updates.len()))
        } else if let Some(v) = &f.ffmpeg_new {
            Some(trf!("у репозиторії новий ffmpeg {v} — перезбери", "new ffmpeg {v} in the repository — rebuild"))
        } else {
            f.self_update()
                .filter(|_| app.self_source.is_some())
                .map(|l| trf!("вийшов OBS-Legacy {}", "OBS-Legacy {} is out", l.trim_start_matches('v')))
        };
        match (active, news) {
            (Some(_), Some(news)) => {
                l.extend(row("⚠", t.warn(), tr!("Оновлення", "Updates"), vec![Span::styled(news, t.warn())]))
            }
            (Some(b), None) if f.obs_pinned.is_some() && versions::newer(&latest, b.obs_version()) => l.extend(row(
                "•",
                t.muted(),
                tr!("Оновлення", "Updates"),
                vec![Span::styled(
                    trf!(
                        "OBS {} зафіксовано, найновіший — {latest}",
                        "OBS {} is pinned, the latest is {latest}",
                        b.obs_version()
                    ),
                    t.muted(),
                )],
            )),
            (Some(b), None) => l.extend(ok(
                tr!("Оновлення", "Updates"),
                trf!("OBS {} — найновіший", "OBS {} is the latest", b.obs_version()),
            )),
            (None, _) => l.extend(row(
                "•",
                t.muted(),
                tr!("Оновлення", "Updates"),
                vec![Span::raw(trf!("найновіший OBS — {latest}", "the latest OBS is {latest}"))],
            )),
        }
    }

    if let Some(r) = app.reports.first() {
        let (icon, style, what) = match r.outcome {
            Outcome::Success if r.warnings > 0 => (
                "✓",
                t.ok(),
                trf!(
                    "готово, {} {}",
                    "done, {} {}",
                    r.warnings,
                    count_word(r.warnings, ["зауваження", "зауваження", "зауважень"], ["note", "notes"])
                ),
            ),
            Outcome::Success => ("✓", t.ok(), tr!("готово", "done").to_string()),
            Outcome::Failed => ("✗", t.err(), tr!("помилка", "error").to_string()),
            Outcome::Cancelled => ("■", t.warn(), tr!("скасовано", "cancelled").to_string()),
            Outcome::Unknown => ("•", t.muted(), tr!("не завершено", "unfinished").to_string()),
        };
        l.extend(row(
            icon,
            style,
            tr!("Останній запуск", "Last run"),
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
            Span::raw(tr!("Збираю відомості про систему…", "Gathering system information…")),
        ])];
    };
    let ok = |s: String| Span::styled(s, t.ok());
    let warn = |s: String| Span::styled(s, t.warn());
    let err = |s: String| Span::styled(s, t.err());
    let muted = |s: String| Span::styled(s, t.muted());
    let bold = |s: String| Span::styled(s, t.strong());
    let mut l: Vec<Line<'static>> = Vec::new();

    if s.is_root {
        l.push(Line::from(err(tr!(
            " ✗ Запущено від root — запусти obs-legacy від звичайного користувача.",
            " ✗ Running as root — start obs-legacy as a regular user."
        )
        .into())));
    }
    if !s.has_pacman {
        l.push(Line::from(warn(
            tr!(
                " ⚠ pacman не знайдено: OBS-Legacy розрахований на Artix / Arch.",
                " ⚠ pacman not found: OBS-Legacy is made for Artix / Arch."
            )
            .into(),
        )));
    }

    l.push(section(t, tr!("Відеокарта і драйвер", "Graphics card and driver")));
    if s.gpus.is_empty() {
        l.push(kv(t, "GPU", vec![warn(tr!("NVIDIA не виявлено", "no NVIDIA detected").into())]));
    } else {
        l.push(kv(t, "GPU", vec![bold(s.gpus.join(", "))]));
    }
    let driver = match (s.driver.clone(), s.driver_major()) {
        (Some(v), Some(580)) => vec![bold(v), ok(tr!("  ✓ гілка 580xx (legacy)", "  ✓ 580xx branch (legacy)").into())],
        (Some(v), Some(m)) if m > 580 => {
            vec![
                bold(v),
                muted(
                    tr!(
                        "  новіший за 580xx — NVENC працює і в звичайному OBS",
                        "  newer than 580xx — NVENC works in the regular OBS too"
                    )
                    .into(),
                ),
            ]
        }
        (Some(v), Some(m)) if m >= 570 => {
            vec![bold(v), ok(tr!("  підтримує NVENC API 13.0", "  supports NVENC API 13.0").into())]
        }
        (Some(v), _) => {
            vec![bold(v), warn(tr!("  застарий для NVENC API 13.0", "  too old for NVENC API 13.0").into())]
        }
        (None, _) => {
            vec![
                err(tr!("не знайдено (модуль nvidia не завантажено?)", "not found (nvidia module not loaded?)").into()),
            ]
        }
    };
    l.push(kv(t, tr!("Драйвер", "Driver"), driver));
    if let Some(p) = &s.driver_pkg {
        l.push(kv(t, tr!("Пакет", "Package"), vec![muted(p.clone())]));
    }
    let api = match (s.nvenc_api.clone(), s.headers_api.clone(), s.nvenc_compatible()) {
        (Some(a), Some(h), Some(true)) => {
            vec![
                bold(a),
                ok(trf!("  ✓ сумісно із заголовками збірки ({h})", "  ✓ compatible with the build headers ({h})")),
            ]
        }
        (Some(a), Some(h), Some(false)) => {
            vec![
                bold(a),
                err(trf!(
                    "  ✗ замало для заголовків {h} — зміни nv-codec-headers",
                    "  ✗ too old for the {h} headers — change nv-codec-headers"
                )),
            ]
        }
        (Some(a), _, _) => vec![bold(a)],
        (None, _, _) => {
            vec![warn(tr!("невідомо (немає libnvidia-encode.so.1)", "unknown (no libnvidia-encode.so.1)").into())]
        }
    };
    l.push(kv(t, "NVENC API", api));

    l.push(Line::from(""));
    l.push(section(t, "OBS-Legacy"));
    let active = s.active_build();
    match active {
        Some(b) => {
            l.push(kv(
                t,
                tr!("Активна збірка", "Active build"),
                vec![bold(format!("OBS {}", b.obs_version())), muted(format!("  · {}", b.built()))],
            ));
            let pkgs = match (&s.legacy_pkg, &s.ffmpeg_pkg) {
                _ if b.legacy => vec![warn(
                    tr!(
                        "стара схема /opt — «Встановити / оновити OBS» поставить пакети",
                        "old /opt layout — “Install / update OBS” will install packages"
                    )
                    .into(),
                )],
                (Some(o), Some(f)) => vec![ok(format!("✓ {o}")), muted(format!(" · {f}"))],
                (Some(o), None) => {
                    vec![ok(format!("✓ {o}")), muted(tr!(" · ffmpeg системний", " · system ffmpeg").into())]
                }
                (None, _) => vec![warn(tr!("не встановлено", "not installed").into())],
            };
            l.push(kv(t, tr!("Пакети", "Packages"), pkgs));
            if let Some(p) = b.profile() {
                let mut spans = vec![bold(p)];
                if let Some(cpu) = s.built_for_other_cpu() {
                    spans.push(warn(trf!(
                        "  ⚠ зібрано під інший процесор ({cpu}) — перезбери",
                        "  ⚠ built for another processor ({cpu}) — rebuild"
                    )));
                } else if b.get("cpu_tuning") == Some("native") {
                    spans.push(muted(tr!("  саме під цей процесор", "  for exactly this processor").into()));
                }
                l.push(kv(t, tr!("Оптимізовано", "Tuned for"), spans));
            }
            match b.get("cef_build") {
                Some(cef) if !cef.is_empty() => {
                    l.push(kv(t, "Browser Source", vec![ok(trf!("✓ вбудовано (CEF {cef})", "✓ built in (CEF {cef})"))]))
                }
                _ => l.push(kv(t, "Browser Source", vec![warn("?".into())])),
            }
            if let Some(sdk) = b.get("nvenc_api_headers") {
                let state = match b.get("nvenc_test") {
                    Some("passed") => {
                        ok(trf!("✓ SDK {sdk}, перевірено obs-nvenc-test", "✓ SDK {sdk}, checked by obs-nvenc-test"))
                    }
                    Some("skipped") => warn(trf!(
                        "SDK {sdk}, тест NVENC пропущено (SKIP_NVENC_TEST)",
                        "SDK {sdk}, NVENC test skipped (SKIP_NVENC_TEST)"
                    )),
                    _ => Span::raw(format!("SDK {sdk}")),
                };
                l.push(kv(t, "NVENC", vec![state]));
            }
            let plugins = b.plugins();
            let mut spans: Vec<Span<'static>> = Vec::new();
            if plugins.is_empty() {
                spans.push(muted(tr!("немає", "none").into()));
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
                    .filter(|latest| versions::newer(latest, ver));
                spans.push(Span::raw(id.clone()));
                spans.push(muted(format!(" {ver}")));
                if let Some(n) = newer {
                    spans.push(warn(format!(" → {n}")));
                }
            }
            for id in b.plugins_failed() {
                spans.push(err(trf!("  ✗ {id} не зібрався", "  ✗ {id} failed to build")));
            }
            l.push(kv(t, tr!("Плагіни", "Plugins"), spans));
        }
        None => l.push(kv(
            t,
            tr!("Активна збірка", "Active build"),
            vec![muted(tr!("не встановлено", "not installed").into())],
        )),
    }
    let obs_cmd = match &s.obs_cmd {
        Some(p) if s.obs_cmd_ok() => vec![ok(format!("✓ {} → OBS-Legacy", p.display()))],
        Some(p) if active.is_some() => {
            vec![warn(trf!("{} — не OBS-Legacy (PATH?)", "{} — not OBS-Legacy (PATH?)", p.display()))]
        }
        Some(p) => vec![muted(format!("{}", p.display()))],
        None => vec![muted(tr!("не знайдено", "not found").into())],
    };
    l.push(kv(t, tr!("Команда obs", "obs command"), obs_cmd));
    let hook = if s.hook_ours {
        vec![ok(tr!("✓ перевіряє бібліотеки після оновлень системи", "✓ checks libraries after system updates").into())]
    } else if active.is_some() {
        vec![warn(tr!("немає", "none").into())]
    } else {
        vec![muted(tr!("з'явиться після встановлення", "appears after installation").into())]
    };
    l.push(kv(t, tr!("pacman-хук", "pacman hook"), hook));
    let notifier = match app.notifier {
        None => vec![muted("…".into())],
        Some(Some(b)) => vec![ok(trf!("✓ увімкнено ({})", "✓ on ({})", b.label()))],
        Some(None) => vec![muted(
            tr!("вимкнено — «Додатково → Сповіщення про оновлення»", "off — “More → Update notifications”").into(),
        )],
    };
    l.push(kv(t, tr!("Сповіщення", "Notifications"), notifier));
    if s.legacy_leftovers && active.is_some_and(|b| !b.legacy) {
        l.push(kv(
            t,
            tr!("Стара схема", "Old layout"),
            vec![warn(
                tr!(
                    "лишилися /opt чи обгортка /usr/local/bin/obs — приберуться під час наступного встановлення",
                    "/opt or the /usr/local/bin/obs wrapper are left — removed during the next installation"
                )
                .into(),
            )],
        ));
    }
    if let Some(p) = &s.obs_pkg {
        let note = if s.legacy_pkg.is_some() {
            tr!(" — інший OBS (pacman мав його замінити)", " — another OBS (pacman should have replaced it)")
        } else {
            tr!(" — заміниться під час встановлення", " — will be replaced during installation")
        };
        l.push(kv(t, tr!("Інший OBS", "Other OBS"), vec![muted(format!("{p}{note}"))]));
    }

    l.push(Line::from(""));
    l.push(section(t, tr!("Система", "System")));
    l.push(kv(t, tr!("ОС", "OS"), vec![Span::raw(s.os.clone()), muted(format!("  · init: {}", s.init))]));
    l.push(kv(t, tr!("Ядро", "Kernel"), vec![Span::raw(s.kernel.clone())]));
    l.push(kv(
        t,
        tr!("Ресурси", "Resources"),
        vec![Span::raw(trf!(
            "{} {} · RAM {:.1} ГіБ вільно з {:.1}",
            "{} {} · RAM {:.1} GiB free of {:.1}",
            s.cpus,
            count_word(s.cpus, ["потік", "потоки", "потоків"], ["thread", "threads"]),
            s.mem_avail_gib,
            s.mem_total_gib
        ))],
    ));
    let space = |v: Option<f64>, need: f64, what: &str| -> Span<'static> {
        match v {
            Some(g) if g < need => warn(trf!(
                "{what} {g:.0} ГіБ (замало, треба ≥{need:.0})",
                "{what} {g:.0} GiB (too little, needs ≥{need:.0})"
            )),
            Some(g) => Span::raw(trf!("{what} {g:.0} ГіБ", "{what} {g:.0} GiB")),
            None => muted(format!("{what} ?")),
        }
    };
    l.push(kv(
        t,
        tr!("Вільне місце", "Free space"),
        vec![
            space(s.free_cache_gib, 10.0, tr!("кеш", "cache")),
            muted(" · ".into()),
            space(s.free_root_gib, 2.0, "/usr"),
        ],
    ));
    l.push(kv(t, tr!("Звіти", "Reports"), vec![muted(truncate(&crate::paths::short_path(&reports::dir()), 60))]));
    l
}
