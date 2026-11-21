//! Фонова підготовка пакетів: коли в репозиторії вийшов новий ffmpeg або після оновлення системи
//! пакетам OBS-Legacy забракло бібліотек, `--watch` сам запускає команду скрипта `prepare` (лише
//! збирає набір — нічого не ставить і нічого не питає) і сповіщає, коли набір готовий. Тоді
//! `obs-legacy --upgrade-system` чи «Встановити / оновити OBS» ставлять його одразу, без збирання.

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

/// Чому готувати пакети — відбиток причин, щоб не збирати двічі під те саме. None — нема чому.
pub fn trigger(ffmpeg_new: Option<&str>, libs: Option<&str>) -> Option<String> {
    let parts: Vec<String> =
        [ffmpeg_new.map(|v| format!("ffmpeg={v}")), libs.map(|f| format!("libs={f}"))].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(" "))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn set(label: &str, complete: bool, active: bool) -> BuildInfo {
        BuildInfo { label: label.into(), complete, active, ..Default::default() }
    }

    #[test]
    fn trigger_joins_reasons() {
        assert_eq!(trigger(None, None), None);
        assert_eq!(trigger(Some("2:9.1-1.1"), None).as_deref(), Some("ffmpeg=2:9.1-1.1"));
        assert_eq!(trigger(Some("2:9.1-2"), Some("ab12")).as_deref(), Some("ffmpeg=2:9.1-2 libs=ab12"));
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
