//! Фонова підготовка пакетів. Коли після оновлення системи пакети OBS-Legacy працюють на збережених
//! копіях старих бібліотек (хук keep-libs) чи їм бракує бібліотек — одразу; коли вийшли новий ffmpeg,
//! OBS чи плагіни — разом з найближчим оновленням системи (транзакцією pacman). Тоді `--watch` сам
//! запускає команду скрипта `prepare` (лише збирає набір — нічого не ставить і нічого не питає) і
//! сповіщає, коли набір готовий; «Встановити / оновити OBS» ставить його одразу, без збирання.

use std::fs::OpenOptions;
use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::builds::{self, BuildInfo};
use crate::paths;
use crate::settings::Settings;

/// Налаштування «Готувати пакети заздалегідь» (типово ввімкнено).
pub const SETTING: &str = "AUTO_PREPARE";

/// Чи ввімкнено фонову підготовку.
pub fn enabled(settings: &Settings) -> bool {
    super::setting(settings, SETTING) != "0"
}

/// Чому готувати пакети. None — цієї причини немає.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reasons {
    /// Нова версія ffmpeg у репозиторії.
    pub ffmpeg: Option<String>,
    /// Пакетам бракує бібліотек (відбиток виводу check-libs).
    pub libs: Option<String>,
    /// Пакети працюють на збережених копіях старих бібліотек (відбиток списку keep-libs).
    pub kept: Option<String>,
    /// Вийшов новий OBS.
    pub obs: Option<String>,
    /// Вийшли нові версії плагінів («Waveform v1.10.0, …»).
    pub plugins: Option<String>,
}

impl Reasons {
    /// Відбиток причин, щоб не збирати двічі під те саме. None — нема чому.
    pub fn key(&self) -> Option<String> {
        let parts: Vec<String> = [
            ("ffmpeg", &self.ffmpeg),
            ("libs", &self.libs),
            ("kept", &self.kept),
            ("obs", &self.obs),
            ("plugins", &self.plugins),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.as_ref().map(|v| format!("{k}={v}")))
        .collect();
        (!parts.is_empty()).then(|| parts.join(" "))
    }
}

/// Блокування скрипта (те саме, що бере `acquire_lock` у скрипті).
fn script_lock() -> PathBuf {
    paths::cache_home().join("obs-studio-nvenc-artix").join(".lock")
}

/// Чи працює зараз скрипт (збирання у вікні TUI чи вже фонова підготовка).
pub fn script_busy() -> bool {
    let Ok(file) = OpenOptions::new().read(true).open(script_lock()) else { return false };
    // SAFETY: flock на власному дескрипторі; блокування знімається, щойно файл закривається
    // (наприкінці цієї функції), тож скрипту воно не заважає.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) != 0 }
}

/// Дочекатися, поки скрипт звільниться (фонова підготовка чи збирання у вікні TUI). true — чекали.
/// Для `obs-legacy --prepare`: пакетний менеджер після оновлення системи не має падати через те, що
/// фонова перевірка саме готує ті самі пакети, — тоді його `prepare` швидко знайде готовий набір.
pub fn wait_script_free() -> bool {
    let Ok(file) = OpenOptions::new().read(true).open(script_lock()) else { return false };
    // SAFETY: flock на власному дескрипторі; спершу без очікування — чи зайнято взагалі.
    let busy = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } != 0;
    if busy {
        // SAFETY: як вище; тепер чекаємо, доки скрипт відпустить блокування (знімається, щойно файл закрито).
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH) };
    }
    busy
}

/// Запустити команду скрипта у фоні з найнижчим пріоритетом (процесор і диск): збирання триває
/// годину й не повинне заважати людині. Вивід не потрібен — скрипт сам пише звіт.
pub fn spawn(script: &Path, command: &str) -> io::Result<Child> {
    let mut cmd = if paths::find_in_path("ionice").is_some() {
        let mut c = Command::new("ionice");
        c.args(["-c", "3", "nice", "-n", "19", "bash"]);
        c
    } else {
        let mut c = Command::new("nice");
        c.args(["-n", "19", "bash"]);
        c
    };
    cmd.arg(script)
        .arg(command)
        .env("OBS_LEGACY_LANG", crate::i18n::lang().code())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Найновіший повний набір, новіший за встановлений, — його можна поставити без збирання.
pub fn ready_label() -> Option<String> {
    let (builds, active) = builds::read_builds();
    ready_in(&builds, active.as_deref())
}

fn ready_in(builds: &[BuildInfo], active: Option<&str>) -> Option<String> {
    let newest = builds.iter().filter(|b| b.complete && !b.legacy && !b.active).map(|b| b.label.as_str()).max()?;
    match active {
        Some(a) if newest <= a => None,
        _ => Some(newest.to_string()),
    }
}

/// Окремо перезібраний ffmpeg-legacy-580xx, ще не встановлений (скрипт: `ffmpeg_only_build`), — його
/// версія. Свіжа збірка — найновіша повна тека сховища, чий час збирання (`built=`, той самий, що в
/// `/usr/share/obs-legacy/ffmpeg.txt` пакета) новіший за встановлений.
pub fn ready_ffmpeg() -> Option<String> {
    let installed = std::fs::read_to_string(crate::system::info_dir().join("ffmpeg.txt")).unwrap_or_default();
    ready_ffmpeg_in(&paths::ffmpeg_store(), &installed)
}

fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix('=')).filter(|v| !v.is_empty())
}

fn ready_ffmpeg_in(store: &Path, installed: &str) -> Option<String> {
    let mut labels: Vec<String> = std::fs::read_dir(store)
        .ok()?
        .flatten()
        .filter(|e| e.path().join(".complete").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    labels.sort();
    let newest = labels.pop()?;
    let info = std::fs::read_to_string(store.join(&newest).join("BUILDINFO.txt")).ok()?;
    let built = field(&info, "built")?;
    // ISO 8601 одного годинника — порівнюються як рядки
    match field(installed, "built") {
        Some(have) if built <= have => None,
        _ => Some(field(&info, "ffmpeg").unwrap_or(&newest).to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ffmpeg_rebuilt_alone_is_ready_until_installed() {
        let store = std::env::temp_dir().join(format!("obs-legacy-ffstore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&store);
        assert_eq!(ready_ffmpeg_in(&store, ""), None, "no store");
        let dir = store.join("20261122-111500_ffmpeg-2_9.1-1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("BUILDINFO.txt"), "kind=ffmpeg\nffmpeg=2:9.1-1\nbuilt=2026-11-22T11:20:00+02:00\n")
            .unwrap();
        assert_eq!(ready_ffmpeg_in(&store, ""), None, "not complete yet");
        std::fs::write(dir.join(".complete"), "").unwrap();
        let older = "arch_version=2:9.1-1.1\nbuilt=2026-11-21T14:50:00+02:00\n";
        assert_eq!(ready_ffmpeg_in(&store, older).as_deref(), Some("2:9.1-1"));
        // встановлено саме цю збірку: той самий час у ffmpeg.txt пакета
        let same = "built=2026-11-22T11:20:00+02:00\n";
        assert_eq!(ready_ffmpeg_in(&store, same), None);
        let _ = std::fs::remove_dir_all(&store);
    }

    fn set(label: &str, complete: bool, active: bool) -> BuildInfo {
        BuildInfo { label: label.into(), complete, active, ..Default::default() }
    }

    #[test]
    fn reasons_key_joins_what_is_there() {
        assert_eq!(Reasons::default().key(), None);
        let r = Reasons { ffmpeg: Some("2:9.1-1.1".into()), ..Default::default() };
        assert_eq!(r.key().as_deref(), Some("ffmpeg=2:9.1-1.1"));
        // Порядок — як у ключах попередніх версій («ffmpeg=… libs=…»): уже спробуване не збирається знову.
        let r = Reasons { ffmpeg: Some("2:9.1-2".into()), libs: Some("ab12".into()), ..Default::default() };
        assert_eq!(r.key().as_deref(), Some("ffmpeg=2:9.1-2 libs=ab12"));
        let r = Reasons { kept: Some("cd34".into()), obs: Some("33.0.3".into()), ..Default::default() };
        assert_eq!(r.key().as_deref(), Some("kept=cd34 obs=33.0.3"));
    }

    #[test]
    fn ready_set_is_newer_and_complete() {
        let installed = set("20261121-114732_33.0.2", true, true);
        let older = set("20261119-152151_33.0.1", true, false);
        let broken = set("20261122-090000_33.0.2", false, false);
        assert_eq!(ready_in(&[installed.clone(), older.clone()], Some(&installed.label)), None);
        // Незавершений набір (збирання обірвалося) — не готовий.
        assert_eq!(ready_in(&[installed.clone(), broken], Some(&installed.label)), None);
        let fresh = set("20261122-100000_33.0.2", true, false);
        assert_eq!(
            ready_in(&[installed.clone(), older, fresh], Some(&installed.label)).as_deref(),
            Some("20261122-100000_33.0.2")
        );
        // Нічого ще не встановлено — готовий будь-який повний набір.
        assert_eq!(
            ready_in(&[set("20261122-100000_33.0.2", true, false)], None).as_deref(),
            Some("20261122-100000_33.0.2")
        );
    }
}
