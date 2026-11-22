//! Де лежать файли: домашня тека, теки XDG (налаштування, стан, кеш, дані),
//! пошук програм у PATH і шляхи для показу людині («~/…»). Лише обчислює
//! шляхи — нічого не створює й не змінює.

use std::env;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn xdg_dir(var: &str, fallback: &str) -> PathBuf {
    match env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home().join(fallback),
    }
}

/// Дані користувача (~/.local/share).
pub fn data_home() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share")
}

/// Дані самої програми: розпакований скрипт, звідки її встановлено.
pub fn data_dir() -> PathBuf {
    data_home().join("obs-legacy")
}

/// Набори зібраних пакетів (для встановлення й відкату) — так само, як у скрипті.
pub fn package_store() -> PathBuf {
    data_home().join("obs-studio-nvenc-artix").join("packages")
}

/// ffmpeg-legacy-580xx, перезібраний окремо від набору (оновлення системи зачепило лише його) — так
/// само, як `FFMPEG_STORE` у скрипті.
pub fn ffmpeg_store() -> PathBuf {
    data_home().join("obs-studio-nvenc-artix").join("ffmpeg-packages")
}

pub fn config_home() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config")
}

pub fn config_dir() -> PathBuf {
    config_home().join("obs-legacy")
}

pub fn cache_home() -> PathBuf {
    xdg_dir("XDG_CACHE_HOME", ".cache")
}

/// Чи запущено від root (`sudo obs-legacy`): тоді служб користувача програма сама не вмикає.
pub fn is_root() -> bool {
    // SAFETY: geteuid не має передумов.
    unsafe { libc::geteuid() == 0 }
}

/// Стан фонової перевірки оновлень (~/.local/state/obs-legacy).
pub fn state_dir() -> PathBuf {
    xdg_dir("XDG_STATE_HOME", ".local/state").join("obs-legacy")
}

/// Виконуваний файл `name` з PATH.
pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

/// «~/x» → «$HOME/x»; інші шляхи — як є.
pub fn expand_home(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(p),
    }
}

/// Шлях для показу: домашня тека — як «~».
pub fn short_path(p: &Path) -> String {
    let home = home();
    match p.strip_prefix(&home) {
        Ok(rest) if home.as_os_str() != "/" => format!("~/{}", rest.display()),
        _ => p.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_paths() {
        let home = home();
        assert_eq!(expand_home("~/.cache/r.txt"), home.join(".cache/r.txt"));
        assert_eq!(expand_home("/tmp/r.txt"), PathBuf::from("/tmp/r.txt"));
        if home.as_os_str() != "/" {
            assert_eq!(short_path(&home.join(".cache/r.txt")), "~/.cache/r.txt");
        }
    }

    #[test]
    fn finds_programs_in_path() {
        assert!(find_in_path("sh").is_some());
        assert!(find_in_path("obs-legacy-no-such-program").is_none());
    }
}
