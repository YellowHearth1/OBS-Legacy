//! Скрипт збирання вбудовано в бінарник, тож версії TUI і скрипта завжди
//! збігаються. Перед запуском він розпаковується у сталий шлях
//! ~/.local/share/obs-legacy/, на який посилаються підказки самого скрипта.

use std::env;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result};

pub const SCRIPT: &str = include_str!("../scripts/build-obs-nvenc-legacy-artix.sh");
pub const SCRIPT_NAME: &str = "build-obs-nvenc-legacy-artix.sh";

/// Версія скрипта з рядка `readonly SCRIPT_VERSION="…"`.
pub fn script_version() -> &'static str {
    SCRIPT
        .lines()
        .find_map(|l| l.strip_prefix("readonly SCRIPT_VERSION=\""))
        .and_then(|rest| rest.split('"').next())
        .unwrap_or("?")
}

pub fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn xdg_dir(var: &str, fallback: &str) -> PathBuf {
    match env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home().join(fallback),
    }
}

pub fn data_dir() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share").join("obs-legacy")
}

/// Набори зібраних пакетів (для встановлення й відкату) — так само, як у скрипті.
pub fn package_store() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share").join("obs-studio-nvenc-artix").join("packages")
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

/// Стан фонової перевірки оновлень (~/.local/state/obs-legacy).
pub fn state_dir() -> PathBuf {
    xdg_dir("XDG_STATE_HOME", ".local/state").join("obs-legacy")
}

/// Тека з вихідними кодами OBS-Legacy, звідки його встановлено (пише install.sh).
pub fn source_dir() -> Option<PathBuf> {
    let text = fs::read_to_string(data_dir().join("source-dir")).ok()?;
    let dir = PathBuf::from(text.trim());
    (dir.join(".git").exists() && dir.join("install.sh").is_file()).then_some(dir)
}

/// Звідки програма оновлює саму себе з меню («Оновити OBS-Legacy»).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelfSource {
    /// Клон репозиторію, з якого її зібрав install.sh: git pull і ./install.sh --update.
    Git(PathBuf),
    /// Готова програма з релізу (install.sh поза вихідними кодами): install.sh з найновішого релізу.
    Release,
}

/// Як оновлювати саму програму, якщо це вміє меню. Пакет pacman (/usr/bin/obs-legacy) — ні:
/// його оновлює pacman.
pub fn self_source() -> Option<SelfSource> {
    if let Some(dir) = source_dir() {
        return Some(SelfSource::Git(dir));
    }
    // install.sh з релізу пише сюди тег; запущено має бути саме ту копію, яку він поставив.
    let installed = fs::canonicalize(home().join(".local/bin/obs-legacy"));
    (data_dir().join("release-install").is_file() && installed.is_ok_and(|p| p == crate::service::exe_path()))
        .then_some(SelfSource::Release)
}

/// Шлях до скрипта, готового до запуску. `OBS_LEGACY_SCRIPT` дозволяє
/// підставити власну копію (для розробки).
pub fn ensure_extracted() -> Result<PathBuf> {
    if let Some(custom) = env::var_os("OBS_LEGACY_SCRIPT") {
        return Ok(PathBuf::from(custom));
    }
    let dir = data_dir();
    fs::create_dir_all(&dir).with_context(|| trf!("не вдалося створити {}", "couldn't create {}", dir.display()))?;
    let path = dir.join(SCRIPT_NAME);
    let current = fs::read(&path).ok();
    if current.as_deref() != Some(SCRIPT.as_bytes()) {
        let tmp = dir.join(format!(".{SCRIPT_NAME}.{}", std::process::id()));
        {
            let mut f = fs::File::create(&tmp)
                .with_context(|| trf!("не вдалося записати {}", "couldn't write {}", tmp.display()))?;
            f.write_all(SCRIPT.as_bytes())?;
            f.sync_all()?;
        }
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
        fs::rename(&tmp, &path).with_context(|| trf!("не вдалося оновити {}", "couldn't update {}", path.display()))?;
    } else {
        let mode = fs::metadata(&path)?.permissions().mode();
        if mode & 0o111 == 0 {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_version_is_parsed() {
        let v = script_version();
        assert_ne!(v, "?");
        assert!(v.chars().next().is_some_and(|c| c.is_ascii_digit()));
    }

    #[test]
    fn script_mentions_attribution() {
        assert!(SCRIPT.contains("Claude Code"));
        assert!(SCRIPT.contains("sl5net/build-obs-nvenc-legacy"));
    }
}
