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

mod commands;
mod control;
mod detect;
mod dinit;
mod files;

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use crate::paths::{self, find_in_path, short_path};
use crate::updates::{self, State};

use commands::{disable_cmd, enable_cmd, print_status, restart_cmd, test_cmd};
pub use detect::{available, installed};

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
            Backend::Systemd => tr!("служба systemd", "systemd service"),
            Backend::Dinit => tr!("служба dinit", "dinit service"),
            Backend::Openrc => tr!("служба OpenRC", "OpenRC service"),
            Backend::Runit => tr!("служба runit", "runit service"),
            Backend::S6 => tr!("служба s6", "s6 service"),
            Backend::Autostart => tr!("автозапуск сесії", "session autostart"),
        }
    }

    /// Файл чи тека служби лежить у фіксованому місці (для runit і s6 — у теці
    /// служб конкретного runsvdir/s6-svscan).
    fn fixed_path(self) -> Option<PathBuf> {
        let config = paths::config_home();
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
        short_path(&self.path)
    }
}

/// Скільки чекати на команду init-системи (dinitctl, systemctl, sv…): зависла не тримає програму вічно.
const COMMAND_TIMEOUT_SECS: &str = "20";

/// Команда з обмеженням часу; Ok — код 0.
fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new("timeout")
        .arg(COMMAND_TIMEOUT_SECS)
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
        let call = format!("{program} {}", args.join(" "));
        Err(if text.is_empty() { trf!("{call}: код {}", "{call}: code {}", out.status) } else { text.into() })
    }
}

fn quiet_ok(program: &str, args: &[&str]) -> bool {
    find_in_path(program).is_some() && run(program, args).is_ok()
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
        let mut s = trf!("Остання перевірка: {}", "Last check: {}", crate::clock::local_time(t));
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

/// Позначка «людина вже вирішила щодо сповіщень» (увімкнула, вимкнула чи відповіла install.sh): після
/// неї програма більше не вмикає їх сама. Цей шлях пише й install.sh.
fn decided_marker() -> PathBuf {
    paths::state_dir().join("notifier-decided")
}

fn mark_decided() {
    // Без позначки найгірше — програма ще раз сама ввімкне сповіщення; це не причина зупиняти команду.
    let _ = fs::create_dir_all(paths::state_dir()).and_then(|()| fs::write(decided_marker(), ""));
}

/// Сповіщення типово ввімкнені: перший запуск меню вмикає фонову перевірку, якщо її ще немає і людина
/// про неї не вирішувала (вимкнути — «Додатково → Сповіщення про оновлення»). Запуск для розробки
/// (`OBS_LEGACY_SCRIPT`) і від root нічого не вмикає. Повертає службу, якщо щойно ввімкнулась.
pub fn enable_by_default() -> Option<Backend> {
    if decided_marker().exists()
        || !installed().is_empty()
        || std::env::var_os("OBS_LEGACY_SCRIPT").is_some()
        || paths::is_root()
    {
        return None;
    }
    mark_decided();
    let ok = Command::new(exe_path())
        .args(["--notifier", "enable"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if ok { installed().first().map(|p| p.backend) } else { None }
}

/// `obs-legacy --notifier [status|enable [служба]|disable|restart|test]`.
pub fn main(args: &[String]) -> ExitCode {
    let backend = args.get(1).map(|s| (s, Backend::from_name(s)));
    if matches!(args.first().map(String::as_str), Some("enable" | "disable")) {
        mark_decided();
    }
    match (args.first().map(String::as_str), backend) {
        (None | Some("status"), None) => print_status(),
        (Some("enable"), None) => enable_cmd(None),
        (Some("enable"), Some((_, Some(b)))) => enable_cmd(Some(b)),
        (Some("restart"), None) => restart_cmd(),
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
            let all = Backend::ALL.map(Backend::name).join(", ");
            eprintln!(
                "obs-legacy: {}",
                trf!("невідома служба «{name}». Можна: {all}.", "unknown service \"{name}\". Possible: {all}.")
            );
            ExitCode::from(2)
        }
        _ => {
            eprintln!(
                "obs-legacy: --notifier status | enable [{}] | disable | restart | test | is-enabled",
                tr!("служба", "service")
            );
            ExitCode::from(2)
        }
    }
}
