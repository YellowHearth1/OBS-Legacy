//! Фонова перевірка оновлень як служба користувача: `obs-legacy --notifier`.
//!
//! Служба запускає `obs-legacy --watch` (див. updates.rs). Що вміємо:
//!   * systemd — служба користувача (~/.config/systemd/user);
//!   * dinit — служба користувача (~/.config/dinit.d; dinit --user від
//!     userspawn (з elogind) чи turnstile (з seatd чи elogind) — обидва беруть
//!     ~/.config/dinit.d/boot.d і типовий сокет);
//!   * OpenRC 0.60+ — служби користувача (~/.config/rc/init.d);
//!   * runit і s6 — тека служб, яку обслуговує runsvdir чи s6-svscan самого
//!     користувача (напр., turnstile з runit: ~/.config/service);
//!   * автозапуск графічної сесії (~/.config/autostart, XDG) — коли init-система
//!     не запускає служб користувача.
//!
//! Нічого не потребує root (від root `--notifier enable` відмовляється). Кожен
//! файл позначено маркером MARKER: чужих файлів з такою самою назвою не чіпаємо.
//!
//! Безпека. Це служби користувача: їх запускає менеджер служб користувача після
//! входу, а не init системи, тож завантаження системи вони не стосуються.
//! Перезапуски після падінь обмежено в кожній init-системі. Найкрихкіший — dinit:
//! зламаний опис служби, на яку посилається boot.d, не дає `dinit --user`
//! запуститися взагалі (разом з pipewire й рештою служб користувача). Тому опис
//! перевіряє `dinit-check` ще до того, як він ляже в ~/.config/dinit.d, а
//! оновлений опис, якого dinit не прийняв, замінюється попереднім.

use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::cli;
use crate::notify;
use crate::script;
use crate::system::find_in_path;
use crate::updates::{self, State};

/// Назва служби в усіх init-системах.
pub const NAME: &str = "obs-legacy-updates";
pub const MARKER: &str = "Managed by obs-legacy --notifier";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Systemd,
    Dinit,
    Openrc,
    Runit,
    S6,
    Autostart,
}

impl Backend {
    pub const ALL: [Backend; 6] =
        [Backend::Systemd, Backend::Dinit, Backend::Openrc, Backend::Runit, Backend::S6, Backend::Autostart];

    pub fn name(self) -> &'static str {
        match self {
            Backend::Systemd => "systemd",
            Backend::Dinit => "dinit",
            Backend::Openrc => "openrc",
            Backend::Runit => "runit",
            Backend::S6 => "s6",
            Backend::Autostart => "autostart",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.name() == s.to_ascii_lowercase())
    }

    /// Для людини: «служба dinit», «автозапуск сесії».
    pub fn label(self) -> &'static str {
        match self {
            Backend::Systemd => "служба systemd",
            Backend::Dinit => "служба dinit",
            Backend::Openrc => "служба OpenRC",
            Backend::Runit => "служба runit",
            Backend::S6 => "служба s6",
            Backend::Autostart => "автозапуск сесії",
        }
    }

    /// Файл чи тека служби лежить у фіксованому місці (для runit і s6 — у теці
    /// служб конкретного runsvdir/s6-svscan).
    fn fixed_path(self) -> Option<PathBuf> {
        let config = script::config_home();
        match self {
            Backend::Systemd => Some(config.join("systemd/user").join(format!("{NAME}.service"))),
            Backend::Dinit => Some(config.join("dinit.d").join(NAME)),
            Backend::Openrc => Some(config.join("rc/init.d").join(NAME)),
            Backend::Autostart => Some(config.join("autostart").join(format!("{NAME}.desktop"))),
            Backend::Runit | Backend::S6 => None,
        }
    }
}

/// Де служба є (або буде): для runit і s6 — тека служби, для інших — файл.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub backend: Backend,
    pub path: PathBuf,
}

impl Place {
    /// Файл, у якому стоїть маркер.
    fn marker_file(&self) -> PathBuf {
        match self.backend {
            Backend::Runit | Backend::S6 => self.path.join("run"),
            _ => self.path.clone(),
        }
    }

    fn ours(&self) -> bool {
        fs::read_to_string(self.marker_file()).is_ok_and(|t| t.contains(MARKER))
    }

    /// Шлях для показу (~ замість домашньої теки).
    pub fn shown(&self) -> String {
        crate::app::short_path(&self.path)
    }
}

// ------------------------------------------------------------- виявлення

/// Команда з обмеженням часу; Ok — код 0.
fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new("timeout")
        .arg("20")
        .arg(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(text.trim().to_string())
    } else {
        let text = text.trim();
        Err(if text.is_empty() { format!("{program} {}: код {}", args.join(" "), out.status) } else { text.into() })
    }
}

fn quiet_ok(program: &str, args: &[&str]) -> bool {
    find_in_path(program).is_some() && run(program, args).is_ok()
}

/// Тека служб з командного рядка runsvdir (`runsvdir [-P] тека [log]`) чи
/// s6-svscan (`s6-svscan [-c n] [-t n] … [тека]`).
fn scan_dir_arg(args: &[String], s6: bool) -> Option<PathBuf> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a.len() > 1 && a.starts_with('-') {
            if s6 && a.len() == 2 && "cCLtdX".contains(&a[1..]) {
                it.next();
            }
            continue;
        }
        return Some(PathBuf::from(a));
    }
    None
}

/// Теки служб, які обслуговують runsvdir чи s6-svscan цього користувача.
fn supervisor_dirs(program: &str) -> Vec<PathBuf> {
    // SAFETY: getuid не має передумов.
    let uid = unsafe { libc::getuid() };
    let mut dirs: Vec<PathBuf> = Vec::new();
    let Ok(rd) = fs::read_dir("/proc") else { return dirs };
    for e in rd.filter_map(Result::ok) {
        let proc_dir = e.path();
        if e.file_name().to_str().is_none_or(|n| !n.bytes().all(|b| b.is_ascii_digit()))
            || fs::metadata(&proc_dir).map(|m| m.uid()).ok() != Some(uid)
        {
            continue;
        }
        let Ok(raw) = fs::read(proc_dir.join("cmdline")) else { continue };
        let args: Vec<String> =
            raw.split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
        if args.first().and_then(|a| Path::new(a).file_name()).is_none_or(|n| n != program) {
            continue;
        }
        let cwd = fs::read_link(proc_dir.join("cwd")).ok();
        let dir = match scan_dir_arg(&args[1..], program == "s6-svscan") {
            Some(d) if d.is_relative() => cwd.map(|c| c.join(d)),
            Some(d) => Some(d),
            // s6-svscan без аргументу обслуговує поточну теку.
            None if program == "s6-svscan" => cwd,
            None => None,
        };
        if let Some(d) = dir
            && !dirs.contains(&d)
        {
            dirs.push(d);
        }
    }
    dirs
}

/// Тека служб на постійному диску в домашній теці (тека в /run зникне після
/// перезавантаження, тож для неї краще автозапуск).
fn persistent_supervisor_dir(program: &str) -> Option<PathBuf> {
    let home = script::home();
    supervisor_dirs(program).into_iter().find(|d| d.starts_with(&home) && d.is_dir())
}

/// Що підходить для цієї системи — від найкращого; автозапуск завжди останній.
pub fn available() -> Vec<Place> {
    let mut out = Vec::new();
    let fixed = |b: Backend| Place { backend: b, path: b.fixed_path().unwrap_or_default() };
    if Path::new("/run/systemd/system").is_dir() && quiet_ok("systemctl", &["--user", "show-environment"]) {
        out.push(fixed(Backend::Systemd));
    }
    if quiet_ok("dinitctl", &["--user", "list"]) {
        out.push(fixed(Backend::Dinit));
    }
    if find_in_path("rc-update").is_some()
        && std::env::var_os("XDG_RUNTIME_DIR").is_some_and(|d| Path::new(&d).join("openrc").is_dir())
    {
        out.push(fixed(Backend::Openrc));
    }
    if find_in_path("sv").is_some()
        && let Some(dir) = persistent_supervisor_dir("runsvdir")
    {
        out.push(Place { backend: Backend::Runit, path: dir.join(NAME) });
    }
    if find_in_path("s6-svc").is_some()
        && let Some(dir) = persistent_supervisor_dir("s6-svscan")
    {
        out.push(Place { backend: Backend::S6, path: dir.join(NAME) });
    }
    out.push(fixed(Backend::Autostart));
    out
}

/// Де служба вже встановлена (файли з маркером).
pub fn installed() -> Vec<Place> {
    let home = script::home();
    let mut out: Vec<Place> = Backend::ALL
        .into_iter()
        .filter_map(|b| Some(Place { backend: b, path: b.fixed_path()? }))
        .filter(Place::ours)
        .collect();
    for (backend, program) in [(Backend::Runit, "runsvdir"), (Backend::S6, "s6-svscan")] {
        let mut dirs = supervisor_dirs(program);
        if backend == Backend::Runit {
            dirs.extend([home.join(".config/service"), home.join("service"), home.join(".local/service")]);
        }
        for d in dirs {
            let p = Place { backend, path: d.join(NAME) };
            if p.ours() && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

// ------------------------------------------------------------- файли служб

/// Шлях у лапках для командного рядка служби, якщо в ньому є пробіли чи лапки.
fn quoted(path: &Path) -> String {
    let s = path.display().to_string();
    if s.chars().any(|c| c.is_whitespace() || c == '"' || c == '\\') {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s
    }
}

/// Шлях у лапках для sh.
fn sh_quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

/// Шлях для рядка Exec= у .desktop (специфікація Desktop Entry: зарезервовані
/// символи — у лапках, а \\ ще й екранується на рівні рядка).
fn desktop_exec(path: &Path) -> String {
    let s = path.display().to_string().replace('%', "%%");
    if !s.chars().any(|c| c.is_whitespace() || "\"'\\><~|&;$*?#()`".contains(c)) {
        return s;
    }
    let mut q = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '`' | '$' => {
                q.push('\\');
                q.push(c);
            }
            '\\' => q.push_str("\\\\\\\\"),
            _ => q.push(c),
        }
    }
    q.push('"');
    q
}

const ABOUT: &str = "OBS-Legacy: сповіщення про нові версії OBS, плагінів і самого OBS-Legacy";

pub fn render(backend: Backend, exe: &Path) -> String {
    let manage =
        "Керувати: obs-legacy → Додатково → «Сповіщення про оновлення» або obs-legacy --notifier status|disable";
    match backend {
        Backend::Systemd => format!(
            "# {MARKER}\n# {ABOUT}.\n# {manage}\n[Unit]\nDescription={ABOUT}\n\
             # Падає раз за разом — після 4 запусків за 10 хв systemd більше не перезапускає.\n\
             StartLimitIntervalSec=600\nStartLimitBurst=4\n\n[Service]\nType=simple\n\
             ExecStart={} --watch\nRestart=on-failure\nRestartSec=60\n\n[Install]\nWantedBy=default.target\n",
            quoted(exe)
        ),
        Backend::Dinit => format!(
            "# {MARKER}\n# {ABOUT}.\n# {manage}\n# (або dinitctl --user status {NAME})\n\
             # Служба користувача: її запускає dinit --user після входу, а не системний dinit.\n\
             type     = process\ncommand  = {} --watch\nrestart  = true\n\
             # Падає раз за разом — після 3 перезапусків за 5 хв dinit більше не перезапускає.\n\
             restart-delay          = 10\nrestart-limit-interval = 300\nrestart-limit-count    = 3\n\
             log-type = buffer\n",
            quoted(exe)
        ),
        Backend::Openrc => format!(
            "#!{}\n# {MARKER}\n# {manage}\ndescription=\"{ABOUT}\"\nsupervisor=supervise-daemon\n\
             command={}\ncommand_args=\"--watch\"\n\
             # Падає раз за разом — після 3 перезапусків за 10 хв supervise-daemon більше не перезапускає.\n\
             respawn_delay=60\nrespawn_max=3\nrespawn_period=600\n",
            find_in_path("openrc-run").map_or_else(|| "/sbin/openrc-run".into(), |p| p.display().to_string()),
            sh_quoted(exe)
        ),
        Backend::Runit | Backend::S6 => {
            format!("#!/bin/sh\n# {MARKER}\n# {ABOUT}.\n# {manage}\nexec {} --watch 2>&1\n", sh_quoted(exe))
        }
        Backend::Autostart => {
            let exec = desktop_exec(exe);
            format!(
                "[Desktop Entry]\n# {MARKER}\n# {manage}\nType=Application\n\
                 Name=OBS-Legacy: сповіщення про оновлення\nName[en]=OBS-Legacy: update notifications\n\
                 Comment=Раз на 12 годин перевіряє нові версії OBS, плагінів і OBS-Legacy\n\
                 Comment[en]=Checks for new OBS, plugin and OBS-Legacy versions every 12 hours\n\
                 Exec={exec} --watch\nIcon=obs-legacy\nTerminal=false\nNoDisplay=true\n\
                 X-GNOME-Autostart-enabled=true\n"
            )
        }
    }
}

/// `finish` для runit і s6: після падіння (а не зупинки) — хвилина паузи перед
/// перезапуском, інакше runsv і s6-supervise перезапускали б щосекунди. $1 — код
/// виходу ./run; зупинку сигналом runit передає як -1, s6 — як 256.
fn render_finish() -> String {
    format!(
        "#!/bin/sh\n# {MARKER}\n# Фонова перевірка впала (а не зупинена) — зачекати хвилину перед перезапуском.\n\
         [ \"$1\" -gt 0 ] 2>/dev/null && [ \"$1\" -lt 256 ] && sleep 60\nexit 0\n"
    )
}

/// Файли теки служби runit чи s6: назва, вміст, права.
fn supervised_files(backend: Backend, exe: &Path) -> Vec<(&'static str, String, u32)> {
    let mut files = vec![("run", render(backend, exe), 0o755), ("finish", render_finish(), 0o755)];
    if backend == Backend::S6 {
        // s6 дає finish 5 с, а далі вбиває: хвилинна пауза має встигнути.
        files.push(("timeout-finish", "70000\n".into(), 0o644));
    }
    files
}

/// Помилки в описі служби dinit за `dinit-check` (у старих версіях — `dinitcheck`).
/// None — перевіряльника немає. Попередження (скажімо, «не знайдено програму»)
/// помилками не вважаються: такий опис dinit однаково прочитає.
fn dinit_errors(dir: &Path) -> Option<Vec<String>> {
    let checker = ["dinit-check", "dinitcheck"].into_iter().find(|p| find_in_path(p).is_some())?;
    let (Ok(text) | Err(text)) = run(checker, &["-d", &dir.display().to_string(), NAME]);
    Some(text.lines().filter(|l| l.starts_with("Service '") && !l.contains("warning:")).map(str::to_string).collect())
}

/// Перевірити опис служби dinit до того, як він ляже в ~/.config/dinit.d (у
/// тимчасовій теці: у нашого опису немає залежностей).
fn dinit_check_body(body: &str) -> Result<(), String> {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        ".{NAME}-check-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    write_file(&dir.join(NAME), body, 0o644)?;
    let errors = dinit_errors(&dir);
    let _ = fs::remove_dir_all(&dir);
    match errors {
        Some(e) if !e.is_empty() => Err(format!("dinit-check знайшов помилки в описі служби: {}", e.join("; "))),
        _ => Ok(()),
    }
}

/// Записи служби в boot.d (так їх роблять `dinitctl enable`, userspawn і turnstile:
/// ~/.config/dinit.d/boot.d; і поруч з нашим файлом, якщо XDG_CONFIG_HOME інший).
fn dinit_boot_links(file: &Path) -> Vec<PathBuf> {
    let mut links = vec![file.with_file_name("boot.d").join(NAME)];
    let home = script::home().join(".config/dinit.d/boot.d").join(NAME);
    if !links.contains(&home) {
        links.push(home);
    }
    links
}

/// Завантажити опис у dinit і додати службу до boot. dinit спершу читає опис,
/// тож зламаного до boot.d не додасть.
fn dinit_load(again: bool) -> Result<(), String> {
    // Опис служби вже завантажено — перечитати (якщо dinit не дає — спершу зупинити).
    if again && run("dinitctl", &["--user", "reload", NAME]).is_err() {
        let _ = run("dinitctl", &["--user", "stop", NAME]);
        run("dinitctl", &["--user", "reload", NAME])?;
    }
    // Додає службу до boot (посилання в boot.d) і запускає; повторно — «already enabled».
    match run("dinitctl", &["--user", "enable", NAME]) {
        Err(e) if !e.contains("already enabled") => Err(e),
        _ => Ok(()),
    }
}

/// Записати файл атомарно (через тимчасовий поруч).
fn write_file(path: &Path, body: &str, mode: u32) -> Result<(), String> {
    let dir = path.parent().ok_or("без теки")?;
    fs::create_dir_all(dir).map_err(|e| format!("не вдалося створити {}: {e}", dir.display()))?;
    let tmp = dir.join(format!(".{NAME}.tmp{}", std::process::id()));
    fs::write(&tmp, body).map_err(|e| format!("не вдалося записати {}: {e}", tmp.display()))?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| format!("не вдалося записати {}: {e}", path.display()))
}

/// «служба dinit» → «Служба dinit» (на початку речення).
fn upper_first(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// Чужий файл з нашою назвою: не перезаписуємо.
fn foreign(p: &Place) -> bool {
    p.marker_file().exists() && !p.ours()
}

// -------------------------------------------------------- увімкнути / вимкнути

/// Дочекатися, поки фонова перевірка запуститься.
fn wait_running(secs: u64) -> Option<u32> {
    let start = Instant::now();
    loop {
        if let Some(pid) = updates::running_pid() {
            return Some(pid);
        }
        if start.elapsed() > Duration::from_secs(secs) {
            return None;
        }
        thread::sleep(Duration::from_millis(250));
    }
}

/// Запустити `--watch` окремо від термінала (для автозапуску — одразу, без
/// повторного входу в сесію).
fn spawn_detached(exe: &Path) -> Result<(), String> {
    let mut cmd = Command::new(exe);
    cmd.arg("--watch").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // SAFETY: setsid безпечний між fork і exec: процес не залежить від термінала TUI.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("не вдалося запустити {} --watch: {e}", exe.display()))
}

/// Зупинити фонову перевірку, якщо вона працює (і це справді наш `--watch`).
fn stop_running() {
    let Some(pid) = updates::running_pid() else { return };
    let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    if !cmdline.split(|b| *b == 0).any(|a| a == b"--watch") {
        return;
    }
    // SAFETY: kill лише надсилає сигнал процесу, PID якого записала фонова перевірка.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    let start = Instant::now();
    while updates::running_pid() == Some(pid) && start.elapsed() < Duration::from_secs(5) {
        thread::sleep(Duration::from_millis(100));
    }
}

/// Записати файл служби й запустити її. `again` — служба вже була: перезапуск.
fn activate(place: &Place, exe: &Path, again: bool) -> Result<(), String> {
    let p = place.path.as_path();
    match place.backend {
        Backend::Systemd => {
            write_file(p, &render(Backend::Systemd, exe), 0o644)?;
            run("systemctl", &["--user", "daemon-reload"])?;
            let unit = format!("{NAME}.service");
            if again {
                run("systemctl", &["--user", "enable", &unit])?;
                run("systemctl", &["--user", "restart", &unit])?;
            } else {
                run("systemctl", &["--user", "enable", "--now", &unit])?;
            }
        }
        Backend::Dinit => {
            let body = render(Backend::Dinit, exe);
            dinit_check_body(&body)?;
            // Опис, який dinit уже прийняв, — повернути, якщо нового він не прийме: на старий
            // посилається boot.d, і зламаний опис там не дав би dinit --user запуститися.
            let previous = if again { fs::read_to_string(p).ok() } else { None };
            write_file(p, &body, 0o644)?;
            if let Err(e) = dinit_load(again) {
                if let Some(old) = previous {
                    let _ = write_file(p, &old, 0o644);
                    let _ = run("dinitctl", &["--user", "reload", NAME]);
                    let _ = run("dinitctl", &["--user", "start", NAME]);
                }
                return Err(e);
            }
            let action =
                if again && run("dinitctl", &["--user", "is-started", NAME]).is_ok() { "restart" } else { "start" };
            run("dinitctl", &["--user", action, NAME])?;
        }
        Backend::Openrc => {
            write_file(p, &render(Backend::Openrc, exe), 0o755)?;
            let _ = run("rc-update", &["--user", "add", NAME, "default"]);
            let action = if again { "restart" } else { "start" };
            run("rc-service", &["--user", NAME, action])?;
        }
        Backend::Runit | Backend::S6 => {
            let files = supervised_files(place.backend, exe);
            if again && p.is_dir() {
                for (name, body, mode) in &files {
                    write_file(&p.join(name), body, *mode)?;
                }
                if place.backend == Backend::Runit {
                    run("sv", &["restart", &p.display().to_string()])?;
                } else {
                    run("s6-svc", &["-r", &p.display().to_string()])?;
                }
            } else {
                // Тека з'являється одразу готовою: runsvdir і s6-svscan не беруть теки з крапкою.
                let parent = p.parent().ok_or("без теки")?;
                let tmp = parent.join(format!(".{NAME}.new"));
                let _ = fs::remove_dir_all(&tmp);
                for (name, body, mode) in &files {
                    write_file(&tmp.join(name), body, *mode)?;
                }
                fs::rename(&tmp, p).map_err(|e| format!("не вдалося створити {}: {e}", p.display()))?;
                if place.backend == Backend::S6 {
                    run("s6-svscanctl", &["-a", &parent.display().to_string()])?;
                }
            }
        }
        Backend::Autostart => {
            write_file(p, &render(Backend::Autostart, exe), 0o644)?;
            stop_running();
            spawn_detached(exe)?;
        }
    }
    Ok(())
}

/// Зупинити й прибрати службу.
fn deactivate(place: &Place) -> Result<(), String> {
    let p = place.path.as_path();
    match place.backend {
        Backend::Systemd => {
            let _ = run("systemctl", &["--user", "disable", "--now", &format!("{NAME}.service")]);
            fs::remove_file(p).map_err(|e| e.to_string())?;
            let _ = run("systemctl", &["--user", "daemon-reload"]);
        }
        Backend::Dinit => {
            let _ = run("dinitctl", &["--user", "disable", NAME]);
            let _ = run("dinitctl", &["--user", "stop", NAME]);
            let _ = run("dinitctl", &["--user", "unload", NAME]);
            // Якщо dinit зараз не працює, disable посилання не прибере — прибрати самому, раніше
            // за опис, щоб не лишити запису без опису. dinit шукає службу з boot.d за назвою
            // запису, тож куди веде посилання — байдуже.
            for link in dinit_boot_links(p) {
                if fs::symlink_metadata(&link).is_ok() {
                    fs::remove_file(&link).map_err(|e| format!("не вдалося прибрати {}: {e}", link.display()))?;
                }
            }
            fs::remove_file(p).map_err(|e| e.to_string())?;
        }
        Backend::Openrc => {
            let _ = run("rc-service", &["--user", NAME, "stop"]);
            let _ = run("rc-update", &["--user", "del", NAME, "default"]);
            fs::remove_file(p).map_err(|e| e.to_string())?;
        }
        Backend::Runit => {
            let dir = p.display().to_string();
            let _ = run("sv", &["down", &dir]);
            let _ = run("sv", &["exit", &dir]);
            fs::remove_dir_all(p).map_err(|e| e.to_string())?;
        }
        Backend::S6 => {
            let dir = p.display().to_string();
            let _ = run("s6-svc", &["-dx", &dir]);
            fs::remove_dir_all(p).map_err(|e| e.to_string())?;
            if let Some(parent) = p.parent() {
                let _ = run("s6-svscanctl", &["-an", &parent.display().to_string()]);
            }
        }
        Backend::Autostart => {
            fs::remove_file(p).map_err(|e| e.to_string())?;
            // Запущену автозапуском перевірку ніхто інший не зупинить.
            stop_running();
        }
    }
    Ok(())
}

/// Поточний виконуваний файл (після оновлення бінарника /proc/self/exe
/// закінчується на « (deleted)»).
pub fn exe_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("obs-legacy"));
    match exe.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
        Some(s) => PathBuf::from(s),
        None => exe,
    }
}

// ----------------------------------------------------------------- стан

pub struct Status {
    pub installed: Vec<Place>,
    pub running: Option<u32>,
    pub last_check: Option<u64>,
    pub summary: Option<String>,
}

impl Status {
    pub fn enabled(&self) -> bool {
        !self.installed.is_empty()
    }

    /// «Остання перевірка: 2026-10-29 14:05 — нового немає».
    pub fn last_line(&self) -> Option<String> {
        let t = self.last_check?;
        let mut s = format!("Остання перевірка: {}", updates::local_time(t));
        if let Some(sum) = &self.summary {
            let _ = write!(s, " — {sum}");
        }
        Some(s)
    }
}

/// Стан без запуску сторонніх команд (швидко: для меню).
pub fn status() -> Status {
    let st = State::load();
    Status {
        installed: installed(),
        running: updates::running_pid(),
        last_check: st.last_check(),
        summary: st.summary().map(str::to_string),
    }
}

// ----------------------------------------------------------------- команди

fn print_status() -> ExitCode {
    let s = status();
    cli::say("Сповіщення про оновлення");
    if s.installed.is_empty() {
        cli::warn("Вимкнено.");
        let best = available().into_iter().next();
        if let Some(b) = best {
            cli::hint(&format!(
                "Увімкнути: obs-legacy → Додатково → «Сповіщення про оновлення» або obs-legacy --notifier enable \
                 ({} підходить для цієї системи).",
                b.backend.label()
            ));
        }
    }
    for p in &s.installed {
        cli::ok(&format!("Увімкнено: {} ({}).", p.backend.label(), p.shown()));
        if p.backend == Backend::Dinit
            && let Some(errors) = p.path.parent().and_then(dinit_errors)
        {
            if errors.is_empty() {
                cli::ok("Опис служби dinit перевірено (dinit-check): помилок немає.");
            } else {
                cli::fail(&format!("dinit-check знайшов помилки в описі служби: {}", errors.join("; ")));
                cli::hint(
                    "Зламаний опис служби в boot.d не дає dinit --user запуститися після входу. Вимкни й увімкни \
                     сповіщення знову: obs-legacy --notifier disable, потім obs-legacy --notifier enable",
                );
            }
        }
    }
    match s.running {
        Some(pid) => cli::ok(&format!("Фонова перевірка працює (PID {pid}).")),
        None if s.enabled() => {
            cli::warn("Фонова перевірка зараз не працює.");
            cli::hint("Вона запуститься після наступного входу в систему; одразу — obs-legacy --notifier restart.");
        }
        None => {}
    }
    if let Some(line) = s.last_line() {
        cli::info(&line);
    }
    cli::info(&format!("Журнал: {}", crate::app::short_path(&updates::log_path())));
    ExitCode::SUCCESS
}

fn send_test() -> Result<String, String> {
    let session = notify::find_session().ok_or("графічної сесії не знайдено (немає WAYLAND_DISPLAY чи DISPLAY)")?;
    notify::send(
        &session,
        &notify::Message {
            title: "OBS-Legacy: сповіщення працюють",
            body: "Так виглядатимуть новини про нові версії OBS, плагінів і самого OBS-Legacy.",
            urgent: false,
            open: None,
        },
    )?;
    Ok(session.describe())
}

fn test_cmd() -> ExitCode {
    cli::say("Пробне сповіщення");
    match send_test() {
        Ok(desc) => {
            cli::ok(&format!("Надіслано ({desc}). Воно має з'явитися на робочому столі."));
            ExitCode::SUCCESS
        }
        Err(e) => {
            cli::fail(&format!("Не вдалося показати сповіщення: {e}."));
            cli::hint(
                "Потрібна служба сповіщень робочого столу (у GNOME, KDE, Xfce вона вбудована; у sway, Hyprland — \
                 mako, dunst чи swaync) і notify-send з пакета libnotify: sudo pacman -S --needed libnotify.",
            );
            ExitCode::FAILURE
        }
    }
}

fn enable_cmd(choice: Option<Backend>, again_only: bool) -> ExitCode {
    // SAFETY: geteuid не має передумов.
    if unsafe { libc::geteuid() } == 0 {
        cli::fail("Сповіщення вмикаються для звичайного користувача, а не для root.");
        cli::hint("Запусти без sudo: obs-legacy --notifier enable");
        return ExitCode::FAILURE;
    }
    let exe = exe_path();
    let installed = installed();
    if again_only {
        // install.sh --update: перезапустити з новою програмою, якщо ввімкнено.
        let Some(place) = installed.first() else { return ExitCode::SUCCESS };
        return match activate(place, &exe, true) {
            Ok(()) => {
                cli::ok(&format!("Фонову перевірку оновлень перезапущено ({}).", place.backend.label()));
                ExitCode::SUCCESS
            }
            Err(e) => {
                cli::warn(&format!("Не вдалося перезапустити фонову перевірку ({}): {e}", place.backend.label()));
                ExitCode::SUCCESS
            }
        };
    }
    cli::say("Вмикаю сповіщення про оновлення");
    let avail = available();
    let target = match choice {
        Some(b) => match avail.iter().find(|p| p.backend == b) {
            Some(p) => p.clone(),
            None => {
                cli::fail(&format!("{} для користувача тут не працює.", upper_first(b.label())));
                let names: Vec<&str> = avail.iter().map(|p| p.backend.name()).collect();
                cli::hint(&format!(
                    "Що працює на цій системі: {}. Без назви програма вибере сама: obs-legacy --notifier enable",
                    names.join(", ")
                ));
                return ExitCode::FAILURE;
            }
        },
        None => avail[0].clone(),
    };
    if foreign(&target) {
        cli::fail(&format!("{} уже існує і створений не OBS-Legacy — не чіпаю.", target.shown()));
        cli::hint("Перейменуй або прибери цей файл і спробуй ще раз.");
        return ExitCode::FAILURE;
    }
    // Одна служба: інші, якщо були, прибрати (наприклад, після зміни init-системи).
    for p in installed.iter().filter(|p| **p != target) {
        match deactivate(p) {
            Ok(()) => cli::ok(&format!("Прибрано попередню: {} ({}).", p.backend.label(), p.shown())),
            Err(e) => cli::warn(&format!("Не вдалося прибрати {}: {e}", p.shown())),
        }
    }
    let again = installed.contains(&target);
    if let Err(e) = activate(&target, &exe, again) {
        // Нова служба не запрацювала — не лишати її напівувімкненою (файл без запуску, запис у boot.d).
        if !again && target.ours() {
            let _ = deactivate(&target);
        }
        cli::fail(&format!("Не вдалося ввімкнути {}: {e}", target.backend.label()));
        if target.backend != Backend::Autostart {
            cli::hint("Спробуй автозапуск сесії замість служби: obs-legacy --notifier enable autostart");
        }
        return ExitCode::FAILURE;
    }
    cli::ok(&format!("{}: {}", upper_first(target.backend.label()), target.shown()));
    match wait_running(10) {
        Some(pid) => cli::ok(&format!("Фонова перевірка працює (PID {pid}): нові версії — раз на 12 годин.")),
        None => cli::warn("Служба ввімкнена, але фонова перевірка ще не відповіла."),
    }
    match send_test() {
        Ok(_) => cli::ok("Надіслав пробне сповіщення — воно має з'явитися на робочому столі."),
        Err(e) => {
            cli::warn(&format!("Пробне сповіщення не показано: {e}."));
            cli::hint(
                "Потрібна служба сповіщень робочого столу (у GNOME, KDE, Xfce вбудована; у sway, Hyprland — mako, \
                 dunst чи swaync) і пакет libnotify.",
            );
        }
    }
    if target.backend == Backend::Autostart {
        cli::warn("Служб користувача init-система тут не запускає, тож перевірку запускає автозапуск сесії.");
        cli::hint(
            "Якщо твій віконний менеджер не виконує ~/.config/autostart (i3, sway, Hyprland…), додай у його \
             конфіг запуск: obs-legacy --watch",
        );
    }
    cli::info("Вимкнути: obs-legacy → Додатково → «Сповіщення про оновлення» або obs-legacy --notifier disable.");
    ExitCode::SUCCESS
}

fn disable_cmd() -> ExitCode {
    cli::say("Вимикаю сповіщення про оновлення");
    let installed = installed();
    let mut code = ExitCode::SUCCESS;
    for p in &installed {
        match deactivate(p) {
            Ok(()) => cli::ok(&format!("Прибрано: {} ({}).", p.backend.label(), p.shown())),
            Err(e) => {
                cli::fail(&format!("Не вдалося прибрати {}: {e}", p.shown()));
                code = ExitCode::FAILURE;
            }
        }
    }
    // Фонова перевірка без служби (запущена вручну чи автозапуском) — теж зупинити.
    stop_running();
    if installed.is_empty() {
        cli::ok("Сповіщення й так вимкнено.");
    } else {
        cli::ok("Сповіщень про оновлення більше не буде.");
    }
    code
}

/// `obs-legacy --notifier [status|enable [служба]|disable|restart|test]`.
pub fn main(args: &[String]) -> ExitCode {
    let backend = args.get(1).map(|s| (s, Backend::from_name(s)));
    match (args.first().map(String::as_str), backend) {
        (None | Some("status"), None) => print_status(),
        (Some("enable"), None) => enable_cmd(None, false),
        (Some("enable"), Some((_, Some(b)))) => enable_cmd(Some(b), false),
        (Some("restart"), None) => enable_cmd(None, true),
        (Some("disable"), None) => disable_cmd(),
        // Для install.sh: код 0 — увімкнено.
        (Some("is-enabled"), None) => {
            if installed().is_empty() {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        (Some("test"), None) => test_cmd(),
        (Some("enable"), Some((name, None))) => {
            eprintln!("obs-legacy: невідома служба «{name}». Можна: {}.", Backend::ALL.map(Backend::name).join(", "));
            ExitCode::from(2)
        }
        _ => {
            eprintln!("obs-legacy: --notifier status | enable [служба] | disable | restart | test | is-enabled");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn scan_dirs_from_cmdline() {
        assert_eq!(
            scan_dir_arg(&args(&["-P", "/home/u/.config/service", "log: ..."]), false),
            Some("/home/u/.config/service".into())
        );
        assert_eq!(scan_dir_arg(&args(&["/home/u/service"]), false), Some("/home/u/service".into()));
        assert_eq!(
            scan_dir_arg(&args(&["-t", "5000", "-d3", "/run/user/1000/s6"]), true),
            Some("/run/user/1000/s6".into())
        );
        assert_eq!(scan_dir_arg(&args(&["-c", "500"]), true), None);
    }

    #[test]
    fn unit_files() {
        let exe = Path::new("/home/u/.local/bin/obs-legacy");
        let systemd = render(Backend::Systemd, exe);
        assert!(systemd.contains("ExecStart=/home/u/.local/bin/obs-legacy --watch\n"));
        assert!(systemd.contains("WantedBy=default.target"));
        assert!(systemd.contains("StartLimitIntervalSec=600\nStartLimitBurst=4\n"));
        let dinit = render(Backend::Dinit, exe);
        assert!(dinit.contains("command  = /home/u/.local/bin/obs-legacy --watch\n"));
        assert!(dinit.contains("restart  = true"));
        assert!(dinit.contains("restart-delay          = 10\n"));
        assert!(dinit.contains("restart-limit-count    = 3\n"));
        let openrc = render(Backend::Openrc, exe);
        assert!(openrc.contains("command='/home/u/.local/bin/obs-legacy'\ncommand_args=\"--watch\""));
        assert!(openrc.contains("respawn_delay=60\nrespawn_max=3\nrespawn_period=600\n"));
        assert!(render(Backend::Runit, exe).contains("exec '/home/u/.local/bin/obs-legacy' --watch 2>&1"));
        let names = |b| supervised_files(b, exe).into_iter().map(|f| f.0).collect::<Vec<_>>();
        assert_eq!(names(Backend::Runit), ["run", "finish"]);
        assert_eq!(names(Backend::S6), ["run", "finish", "timeout-finish"]);
        let desktop = render(Backend::Autostart, exe);
        assert!(desktop.starts_with("[Desktop Entry]\n"));
        assert!(desktop.contains("Exec=/home/u/.local/bin/obs-legacy --watch\n"));
        for b in Backend::ALL {
            assert!(render(b, exe).contains(MARKER), "{b:?}");
            assert_eq!(Backend::from_name(b.name()), Some(b));
        }
    }

    #[test]
    fn autostart_entry_is_valid() {
        // desktop-file-validate є не всюди (пакет desktop-file-utils) — тоді лише формат вище.
        let Some(validator) = find_in_path("desktop-file-validate") else { return };
        let file = std::env::temp_dir().join(format!("{NAME}-{}.desktop", std::process::id()));
        fs::write(&file, render(Backend::Autostart, Path::new("/home/u/My Apps/obs-legacy"))).unwrap();
        let out = Command::new(validator).arg(&file).output().unwrap();
        let _ = fs::remove_file(&file);
        let report = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success() && !report.contains("error"), "{report}");
    }

    /// Справжній виконуваний файл: перевіряльники попереджають, якщо програми немає.
    fn real_exe() -> PathBuf {
        std::env::current_exe().unwrap()
    }

    #[test]
    fn dinit_description_is_valid() {
        // dinit-check є лише там, де стоїть dinit; без нього перевіряє сам dinit під час enable.
        if !["dinit-check", "dinitcheck"].iter().any(|p| find_in_path(p).is_some()) {
            return;
        }
        assert_eq!(dinit_check_body(&render(Backend::Dinit, &real_exe())), Ok(()));
        let broken = "type = process\ncommand = /bin/true\nrestart = maybe\n";
        assert!(dinit_check_body(broken).unwrap_err().contains("restart"));
    }

    #[test]
    fn shell_scripts_parse() {
        let exe = Path::new("/home/u/My Apps/it's/obs-legacy");
        let mut scripts = vec![render(Backend::Openrc, exe)];
        for b in [Backend::Runit, Backend::S6] {
            scripts.extend(supervised_files(b, exe).into_iter().filter(|f| f.2 == 0o755).map(|f| f.1));
        }
        for body in scripts {
            let out = Command::new("sh").args(["-n", "-c", &body]).output().unwrap();
            assert!(out.status.success(), "{body}\n{}", String::from_utf8_lossy(&out.stderr));
        }
    }

    #[test]
    fn finish_pauses_only_after_crash() {
        let script = render_finish().replace("sleep 60", "echo pause");
        let finish = |code: &str| {
            let out = Command::new("sh").args(["-c", &script, "finish", code, "0"]).output().unwrap();
            assert!(out.status.success());
            String::from_utf8_lossy(&out.stdout).contains("pause")
        };
        // Впала (код 1, паніка 101, програми немає 127) — пауза; зупинили сигналом чи вийшла сама — ні.
        assert!(finish("1") && finish("101") && finish("127"));
        assert!(!finish("0") && !finish("-1") && !finish("256"));
    }

    #[test]
    fn systemd_unit_is_valid() {
        // systemd-analyze є лише там, де systemd (наприклад, у CI на Ubuntu).
        let Some(analyze) = find_in_path("systemd-analyze") else { return };
        let dir = std::env::temp_dir().join(format!("{NAME}-unit-{}", std::process::id()));
        let unit = dir.join(format!("{NAME}.service"));
        write_file(&unit, &render(Backend::Systemd, &real_exe()), 0o644).unwrap();
        let out = Command::new(analyze).arg("verify").arg(&unit).output().unwrap();
        let _ = fs::remove_dir_all(&dir);
        let report = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success() && !report.contains(NAME), "{report}");
    }

    #[test]
    fn paths_with_spaces_are_quoted() {
        let exe = Path::new("/home/u/My Apps/obs-legacy");
        assert!(render(Backend::Systemd, exe).contains("ExecStart=\"/home/u/My Apps/obs-legacy\" --watch"));
        assert!(render(Backend::Dinit, exe).contains("command  = \"/home/u/My Apps/obs-legacy\" --watch"));
        assert!(render(Backend::Runit, exe).contains("exec '/home/u/My Apps/obs-legacy' --watch"));
        assert!(render(Backend::Autostart, exe).contains("Exec=\"/home/u/My Apps/obs-legacy\" --watch"));
        assert_eq!(sh_quoted(Path::new("/a/it's")), r"'/a/it'\''s'");
    }
}
