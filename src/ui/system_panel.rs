//! Панель «Твоя система» на головному екрані: коротко — по рядку на те, що важливо новачкові, докладно
//! (клавіша d) — усе, що відомо про систему й збірки.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::{kv, lower_first, panel, section, spinner, truncate, wrap_spans};
use crate::app::{App, Target};
use crate::builds::BuildInfo;
use crate::i18n::count_word;
use crate::reports::{self, Outcome};
use crate::system::SysInfo;
use crate::theme::Theme;
use crate::versions;

/// Скільки вільного місця в кеші потрібно для першого збирання (так перевіряє скрипт: `check_disk_space`).
const BUILD_FREE_GIB: f64 = 10.0;
/// Скільки вільного місця на розділі з /usr потрібно пакетам (там само).
const USR_FREE_GIB: f64 = 2.0;
/// Найстаріша гілка драйвера, що знає NVENC API 13.0.
const NVENC_13_DRIVER: u32 = 570;

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
    let lines = match &app.sys {
        Some(s) if app.details => dashboard(app, s),
        Some(s) => summary(app, s, inner.width),
        None => gathering(app),
    };
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// Поки відомості про систему ще збираються.
fn gathering(app: &App) -> Vec<Line<'static>> {
    vec![Line::from(vec![
        Span::styled(format!(" {} ", spinner(app.tick)), Style::new().fg(app.theme.accent2)),
        Span::raw(tr!("Збираю відомості про систему…", "Gathering system information…")),
    ])]
}

// ------------------------------------------------------------------ коротко

/// Рядки короткої панелі: « ✓ Назва           значення», довге значення переноситься під себе.
struct Rows<'a> {
    t: &'a Theme,
    width: u16,
    lines: Vec<Line<'static>>,
}

impl Rows<'_> {
    /// Ширина колонки з назвами.
    const LABEL_W: usize = 16;

    fn row(&mut self, icon: &str, style: Style, label: &str, value: Vec<Span<'static>>) {
        let avail = usize::from(self.width).saturating_sub(3 + Self::LABEL_W);
        for (i, mut spans) in wrap_spans(value, avail).into_iter().enumerate() {
            let mut line = if i == 0 {
                vec![
                    Span::styled(format!(" {icon} "), style),
                    Span::styled(format!("{label:<width$}", width = Self::LABEL_W), self.t.muted()),
                ]
            } else {
                vec![Span::raw(" ".repeat(3 + Self::LABEL_W))]
            };
            line.append(&mut spans);
            self.lines.push(Line::from(line));
        }
    }

    /// ✓ — усе гаразд.
    fn ok(&mut self, label: &str, value: String) {
        self.row("✓", self.t.ok(), label, vec![Span::raw(value)]);
    }

    /// ⚠ — варто знати чи зробити.
    fn warn(&mut self, label: &str, value: String) {
        self.row("⚠", self.t.warn(), label, vec![Span::styled(value, self.t.warn())]);
    }

    /// ✗ — так не запрацює.
    fn err(&mut self, label: &str, value: String) {
        self.row("✗", self.t.err(), label, vec![Span::styled(value, self.t.err())]);
    }

    /// • — просто до відома.
    fn note(&mut self, label: &str, value: String) {
        self.row("•", self.t.muted(), label, vec![Span::styled(value, self.t.muted())]);
    }
}

/// Коротко: по рядку на те, що важливо новачкові.
fn summary(app: &App, s: &SysInfo, width: u16) -> Vec<Line<'static>> {
    let mut rows = Rows { t: &app.theme, width, lines: Vec::new() };
    hardware_rows(&mut rows, s);
    packages_row(&mut rows, app);
    build_rows(&mut rows, s);
    plugins_row(&mut rows, app, s.active_build());
    updates_row(&mut rows, app, s.active_build());
    last_run_row(&mut rows, app);
    rows.lines
}

/// Відеокарта, драйвер, NVENC і вільне місце.
fn hardware_rows(rows: &mut Rows, s: &SysInfo) {
    match s.gpus.first() {
        Some(g) => rows.ok(tr!("Відеокарта", "Graphics card"), g.trim_start_matches("NVIDIA ").to_string()),
        None => rows.err(tr!("Відеокарта", "Graphics card"), tr!("NVIDIA не знайдено", "no NVIDIA found").into()),
    }
    match (s.driver.clone(), s.driver_major()) {
        (Some(v), Some(580)) => rows.ok(tr!("Драйвер", "Driver"), trf!("{v} · гілка 580xx", "{v} · 580xx branch")),
        (Some(v), Some(m)) if m > 580 => rows.row(
            "•",
            rows.t.muted(),
            tr!("Драйвер", "Driver"),
            vec![Span::raw(v), Span::styled(tr!(" · новіший за 580xx", " · newer than 580xx"), rows.t.muted())],
        ),
        (Some(v), _) => rows.ok(tr!("Драйвер", "Driver"), v),
        (None, _) => rows.err(tr!("Драйвер", "Driver"), tr!("не завантажено", "not loaded").into()),
    }
    match (s.nvenc_api.clone(), s.nvenc_compatible()) {
        (Some(a), Some(true) | None) => rows.ok("NVENC", trf!("API {a} — підходить", "API {a} — fine")),
        (Some(a), Some(false)) => rows.err("NVENC", trf!("API {a} — замало", "API {a} — too old")),
        (None, _) => rows.warn("NVENC", tr!("невідомо", "unknown").into()),
    }
    let free = s.free_cache_gib.into_iter().chain(s.free_root_gib).fold(f64::INFINITY, f64::min);
    if free.is_finite() {
        if free < BUILD_FREE_GIB {
            rows.warn(
                tr!("Диск", "Disk"),
                trf!("вільно {free:.0} ГіБ — треба ≥ 10", "{free:.0} GiB free — needs ≥ 10"),
            );
        } else {
            rows.ok(tr!("Диск", "Disk"), trf!("вільно {free:.0} ГіБ", "{free:.0} GiB free"));
        }
    }
}

/// Пакети для збірки: чи всі на місці (за `deps --missing`).
fn packages_row(rows: &mut Rows, app: &App) {
    match app.missing_packages.as_deref() {
        Some([]) if app.missing_norepo.is_empty() => {
            rows.ok(tr!("Пакети", "Packages"), tr!("усі на місці", "all in place").into())
        }
        Some(m) if !m.is_empty() => {
            rows.warn(tr!("Пакети", "Packages"), trf!("бракує {}: {}", "{} missing: {}", m.len(), m.join(", ")))
        }
        Some(_) => rows.err(
            tr!("Пакети", "Packages"),
            trf!("немає в репозиторіях: {}", "not in the repositories: {}", app.missing_norepo.join(", ")),
        ),
        None => {}
    }
}

/// Встановлена збірка: версія, Browser Source, під що оптимізовано, куди веде команда obs.
fn build_rows(rows: &mut Rows, s: &SysInfo) {
    let Some(b) = s.active_build() else {
        rows.note(tr!("OBS з NVENC", "OBS with NVENC"), tr!("ще не встановлено", "not installed yet").into());
        return;
    };
    rows.ok(
        tr!("OBS з NVENC", "OBS with NVENC"),
        trf!("OBS {} · зібрано {}", "OBS {} · built {}", b.obs_version(), b.built()),
    );
    if b.get("cef_build").is_none_or(str::is_empty) {
        rows.warn("Browser Source", tr!("невідомо", "unknown").into());
    }
    match (b.legacy, b.profile()) {
        (true, _) => rows.warn(
            tr!("Схема", "Layout"),
            tr!("стара (/opt) — онови, щоб отримати пакети pacman", "old (/opt) — update to get pacman packages")
                .into(),
        ),
        (false, Some(p)) => rows.ok(tr!("Оптимізовано", "Tuned for"), p),
        (false, None) => {}
    }
    if s.obs_cmd_ok() {
        rows.ok(tr!("Команда obs", "obs command"), tr!("відкриває OBS з NVENC", "opens OBS with NVENC").into());
    } else {
        rows.warn(
            tr!("Команда obs", "obs command"),
            tr!("веде не на OBS-Legacy (PATH?)", "doesn't lead to OBS-Legacy (PATH?)").into(),
        );
    }
}

/// Вибрані плагіни з версіями у збірці; ще не встановлені — «буде додано».
fn plugins_row(rows: &mut Rows, app: &App, active: Option<&BuildInfo>) {
    let t = rows.t;
    let selected = app.settings.plugins();
    let installed = active.map(BuildInfo::plugins).unwrap_or_default();
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
    let (icon, style) = if active.is_some() { ("✓", t.ok()) } else { ("•", t.muted()) };
    rows.row(icon, style, tr!("Плагіни", "Plugins"), plugins);
}

/// Нові версії OBS, плагінів, ffmpeg і OBS-Legacy (або що встановлено найновіше).
fn updates_row(rows: &mut Rows, app: &App, active: Option<&BuildInfo>) {
    let label = tr!("Оновлення", "Updates");
    if app.updates_loading {
        rows.row(
            spinner(app.tick),
            Style::new().fg(rows.t.accent2),
            label,
            vec![Span::styled(tr!("перевіряю…", "checking…"), rows.t.muted())],
        );
        return;
    }
    let Some(f) = app.facts() else { return };
    let Some(latest) = f.obs_latest.clone() else { return };
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
        (Some(_), Some(news)) => rows.warn(label, news),
        (Some(b), None) if f.obs_pinned.is_some() && versions::newer(&latest, b.obs_version()) => rows.note(
            label,
            trf!(
                "OBS {} зафіксовано, найновіший — {latest}",
                "OBS {} is pinned, the latest is {latest}",
                b.obs_version()
            ),
        ),
        (Some(b), None) => rows.ok(label, trf!("OBS {} — найновіший", "OBS {} is the latest", b.obs_version())),
        (None, _) => rows.row(
            "•",
            rows.t.muted(),
            label,
            vec![Span::raw(trf!("найновіший OBS — {latest}", "the latest OBS is {latest}"))],
        ),
    }
}

/// Чим закінчився останній запуск скрипта.
fn last_run_row(rows: &mut Rows, app: &App) {
    let t = rows.t;
    let Some(r) = app.reports.first() else { return };
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
    rows.row(
        icon,
        style,
        tr!("Останній запуск", "Last run"),
        vec![
            Span::raw(format!("{} · {}", lower_first(r.label()), reports::when(&r.stamp))),
            Span::styled(format!(" — {what}"), style),
        ],
    );
}

// ------------------------------------------------------------------ докладно

/// Фрагменти тексту в кольорах теми.
struct Paint<'a>(&'a Theme);

impl Paint<'_> {
    fn ok(&self, s: impl Into<String>) -> Span<'static> {
        Span::styled(s.into(), self.0.ok())
    }

    fn warn(&self, s: impl Into<String>) -> Span<'static> {
        Span::styled(s.into(), self.0.warn())
    }

    fn err(&self, s: impl Into<String>) -> Span<'static> {
        Span::styled(s.into(), self.0.err())
    }

    fn muted(&self, s: impl Into<String>) -> Span<'static> {
        Span::styled(s.into(), self.0.muted())
    }

    fn bold(&self, s: impl Into<String>) -> Span<'static> {
        Span::styled(s.into(), self.0.strong())
    }
}

/// Докладно: усе, що відомо про систему й збірки (для досвідчених).
fn dashboard(app: &App, s: &SysInfo) -> Vec<Line<'static>> {
    let t = &app.theme;
    let mut l = environment_warnings(t, s);
    l.extend(gpu_section(t, s));
    l.push(Line::from(""));
    l.extend(obs_legacy_section(app, s));
    l.push(Line::from(""));
    l.extend(system_section(t, s));
    l
}

/// Запуск від root і система без pacman — одразу вгорі.
fn environment_warnings(t: &Theme, s: &SysInfo) -> Vec<Line<'static>> {
    let p = Paint(t);
    let mut l = Vec::new();
    if s.is_root {
        l.push(Line::from(p.err(tr!(
            " ✗ Запущено від root — запусти obs-legacy від звичайного користувача.",
            " ✗ Running as root — start obs-legacy as a regular user."
        ))));
    }
    if !s.has_pacman {
        l.push(Line::from(p.warn(tr!(
            " ⚠ pacman не знайдено: OBS-Legacy розрахований на Artix / Arch.",
            " ⚠ pacman not found: OBS-Legacy is made for Artix / Arch."
        ))));
    }
    l
}

/// Відеокарта, драйвер і NVENC API.
fn gpu_section(t: &Theme, s: &SysInfo) -> Vec<Line<'static>> {
    let p = Paint(t);
    let mut l = vec![section(t, tr!("Відеокарта і драйвер", "Graphics card and driver"))];
    if s.gpus.is_empty() {
        l.push(kv(t, "GPU", vec![p.warn(tr!("NVIDIA не виявлено", "no NVIDIA detected"))]));
    } else {
        l.push(kv(t, "GPU", vec![p.bold(s.gpus.join(", "))]));
    }
    let driver = match (s.driver.clone(), s.driver_major()) {
        (Some(v), Some(580)) => vec![p.bold(v), p.ok(tr!("  ✓ гілка 580xx (legacy)", "  ✓ 580xx branch (legacy)"))],
        (Some(v), Some(m)) if m > 580 => vec![
            p.bold(v),
            p.muted(tr!(
                "  новіший за 580xx — NVENC працює і в звичайному OBS",
                "  newer than 580xx — NVENC works in the regular OBS too"
            )),
        ],
        (Some(v), Some(m)) if m >= NVENC_13_DRIVER => {
            vec![p.bold(v), p.ok(tr!("  підтримує NVENC API 13.0", "  supports NVENC API 13.0"))]
        }
        (Some(v), _) => vec![p.bold(v), p.warn(tr!("  застарий для NVENC API 13.0", "  too old for NVENC API 13.0"))],
        (None, _) => {
            vec![p.err(tr!("не знайдено (модуль nvidia не завантажено?)", "not found (nvidia module not loaded?)"))]
        }
    };
    l.push(kv(t, tr!("Драйвер", "Driver"), driver));
    if let Some(pkg) = &s.driver_pkg {
        l.push(kv(t, tr!("Пакет", "Package"), vec![p.muted(pkg.clone())]));
    }
    let api = match (s.nvenc_api.clone(), s.headers_api.clone(), s.nvenc_compatible()) {
        (Some(a), Some(h), Some(true)) => vec![
            p.bold(a),
            p.ok(trf!("  ✓ сумісно із заголовками збірки ({h})", "  ✓ compatible with the build headers ({h})")),
        ],
        (Some(a), Some(h), Some(false)) => vec![
            p.bold(a),
            p.err(trf!(
                "  ✗ замало для заголовків {h} — зміни nv-codec-headers",
                "  ✗ too old for the {h} headers — change nv-codec-headers"
            )),
        ],
        (Some(a), _, _) => vec![p.bold(a)],
        (None, _, _) => {
            vec![p.warn(tr!("невідомо (немає libnvidia-encode.so.1)", "unknown (no libnvidia-encode.so.1)"))]
        }
    };
    l.push(kv(t, "NVENC API", api));
    l
}

/// Збірка OBS-Legacy, команда obs, pacman-хук, сповіщення, залишки старої схеми й інший OBS.
fn obs_legacy_section(app: &App, s: &SysInfo) -> Vec<Line<'static>> {
    let t = &app.theme;
    let p = Paint(t);
    let mut l = vec![section(t, "OBS-Legacy")];
    let active = s.active_build();
    match active {
        Some(b) => l.extend(active_build_lines(app, s, b)),
        None => {
            l.push(kv(t, tr!("Активна збірка", "Active build"), vec![p.muted(tr!("не встановлено", "not installed"))]))
        }
    }
    let obs_cmd = match &s.obs_cmd {
        Some(cmd) if s.obs_cmd_ok() => vec![p.ok(format!("✓ {} → OBS-Legacy", cmd.display()))],
        Some(cmd) if active.is_some() => {
            vec![p.warn(trf!("{} — не OBS-Legacy (PATH?)", "{} — not OBS-Legacy (PATH?)", cmd.display()))]
        }
        Some(cmd) => vec![p.muted(format!("{}", cmd.display()))],
        None => vec![p.muted(tr!("не знайдено", "not found"))],
    };
    l.push(kv(t, tr!("Команда obs", "obs command"), obs_cmd));
    let hook = if s.hook_ours {
        vec![p.ok(tr!("✓ перевіряє бібліотеки після оновлень системи", "✓ checks libraries after system updates"))]
    } else if active.is_some() {
        vec![p.warn(tr!("немає", "none"))]
    } else {
        vec![p.muted(tr!("з'явиться після встановлення", "appears after installation"))]
    };
    l.push(kv(t, tr!("pacman-хук", "pacman hook"), hook));
    let notifier =
        match app.notifier {
            None => vec![p.muted("…")],
            Some(Some(b)) => vec![p.ok(trf!("✓ увімкнено ({})", "✓ on ({})", b.label()))],
            Some(None) => {
                vec![p.muted(tr!(
                    "вимкнено — «Додатково → Сповіщення про оновлення»",
                    "off — “More → Update notifications”"
                ))]
            }
        };
    l.push(kv(t, tr!("Сповіщення", "Notifications"), notifier));
    if s.legacy_leftovers && active.is_some_and(|b| !b.legacy) {
        l.push(kv(
            t,
            tr!("Стара схема", "Old layout"),
            vec![p.warn(tr!(
                "лишилися /opt чи обгортка /usr/local/bin/obs — приберуться під час наступного встановлення",
                "/opt or the /usr/local/bin/obs wrapper are left — removed during the next installation"
            ))],
        ));
    }
    if let Some(pkg) = &s.obs_pkg {
        let note = if s.legacy_pkg.is_some() {
            tr!(" — інший OBS (pacman мав його замінити)", " — another OBS (pacman should have replaced it)")
        } else {
            tr!(" — заміниться під час встановлення", " — will be replaced during installation")
        };
        l.push(kv(t, tr!("Інший OBS", "Other OBS"), vec![p.muted(format!("{pkg}{note}"))]));
    }
    l
}

/// Активна збірка: пакети, під що оптимізовано, Browser Source, NVENC і плагіни.
fn active_build_lines(app: &App, s: &SysInfo, b: &BuildInfo) -> Vec<Line<'static>> {
    let t = &app.theme;
    let p = Paint(t);
    let mut l = vec![kv(
        t,
        tr!("Активна збірка", "Active build"),
        vec![p.bold(format!("OBS {}", b.obs_version())), p.muted(format!("  · {}", b.built()))],
    )];
    let pkgs = match (&s.legacy_pkg, &s.ffmpeg_pkg) {
        _ if b.legacy => vec![p.warn(tr!(
            "стара схема /opt — «Встановити / оновити OBS» поставить пакети",
            "old /opt layout — “Install / update OBS” will install packages"
        ))],
        (Some(o), Some(f)) => vec![p.ok(format!("✓ {o}")), p.muted(format!(" · {f}"))],
        (Some(o), None) => vec![p.ok(format!("✓ {o}")), p.muted(tr!(" · ffmpeg системний", " · system ffmpeg"))],
        (None, _) => vec![p.warn(tr!("не встановлено", "not installed"))],
    };
    l.push(kv(t, tr!("Пакети", "Packages"), pkgs));
    if let Some(profile) = b.profile() {
        let mut spans = vec![p.bold(profile)];
        if let Some(cpu) = s.built_for_other_cpu() {
            spans.push(p.warn(trf!(
                "  ⚠ зібрано під інший процесор ({cpu}) — перезбери",
                "  ⚠ built for another processor ({cpu}) — rebuild"
            )));
        } else if b.get("cpu_tuning") == Some("native") {
            spans.push(p.muted(tr!("  саме під цей процесор", "  for exactly this processor")));
        }
        l.push(kv(t, tr!("Оптимізовано", "Tuned for"), spans));
    }
    match b.get("cef_build") {
        Some(cef) if !cef.is_empty() => {
            l.push(kv(t, "Browser Source", vec![p.ok(trf!("✓ вбудовано (CEF {cef})", "✓ built in (CEF {cef})"))]))
        }
        _ => l.push(kv(t, "Browser Source", vec![p.warn("?")])),
    }
    if let Some(sdk) = b.get("nvenc_api_headers") {
        let state = match b.get("nvenc_test") {
            Some("passed") => {
                p.ok(trf!("✓ SDK {sdk}, перевірено obs-nvenc-test", "✓ SDK {sdk}, checked by obs-nvenc-test"))
            }
            Some("skipped") => p.warn(trf!(
                "SDK {sdk}, тест NVENC пропущено (SKIP_NVENC_TEST)",
                "SDK {sdk}, NVENC test skipped (SKIP_NVENC_TEST)"
            )),
            _ => Span::raw(format!("SDK {sdk}")),
        };
        l.push(kv(t, "NVENC", vec![state]));
    }
    l.push(kv(t, tr!("Плагіни", "Plugins"), build_plugins(app, b)));
    l
}

/// Плагіни збірки з версіями, нові релізи (→) і ті, що не зібралися.
fn build_plugins(app: &App, b: &BuildInfo) -> Vec<Span<'static>> {
    let p = Paint(&app.theme);
    let plugins = b.plugins();
    let mut spans: Vec<Span<'static>> = Vec::new();
    if plugins.is_empty() {
        spans.push(p.muted(tr!("немає", "none")));
    }
    for (i, (id, ver)) in plugins.iter().enumerate() {
        if i > 0 {
            spans.push(p.muted(", "));
        }
        let newer = app
            .updates
            .as_ref()
            .and_then(|u| u.plugins.iter().find(|(pid, _)| pid == id))
            .and_then(|(_, latest)| latest.clone())
            .filter(|latest| versions::newer(latest, ver));
        spans.push(Span::raw(id.clone()));
        spans.push(p.muted(format!(" {ver}")));
        if let Some(n) = newer {
            spans.push(p.warn(format!(" → {n}")));
        }
    }
    for id in b.plugins_failed() {
        spans.push(p.err(trf!("  ✗ {id} не зібрався", "  ✗ {id} failed to build")));
    }
    spans
}

/// ОС, ядро, ресурси, вільне місце й тека звітів.
fn system_section(t: &Theme, s: &SysInfo) -> Vec<Line<'static>> {
    let p = Paint(t);
    let space = |v: Option<f64>, need: f64, what: &str| -> Span<'static> {
        match v {
            Some(g) if g < need => p.warn(trf!(
                "{what} {g:.0} ГіБ (замало, треба ≥{need:.0})",
                "{what} {g:.0} GiB (too little, needs ≥{need:.0})"
            )),
            Some(g) => Span::raw(trf!("{what} {g:.0} ГіБ", "{what} {g:.0} GiB")),
            None => p.muted(format!("{what} ?")),
        }
    };
    vec![
        section(t, tr!("Система", "System")),
        kv(t, tr!("ОС", "OS"), vec![Span::raw(s.os.clone()), p.muted(format!("  · init: {}", s.init))]),
        kv(t, tr!("Ядро", "Kernel"), vec![Span::raw(s.kernel.clone())]),
        kv(
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
        ),
        kv(
            t,
            tr!("Вільне місце", "Free space"),
            vec![
                space(s.free_cache_gib, BUILD_FREE_GIB, tr!("кеш", "cache")),
                p.muted(" · "),
                space(s.free_root_gib, USR_FREE_GIB, "/usr"),
            ],
        ),
        kv(t, tr!("Звіти", "Reports"), vec![p.muted(truncate(&crate::paths::short_path(&reports::dir()), 60))]),
    ]
}
