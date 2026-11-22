//! Скрипт збирання вбудовано в бінарник, тож версії TUI і скрипта завжди
//! збігаються. Перед запуском він розпаковується у сталий шлях
//! ~/.local/share/obs-legacy/, на який посилаються підказки самого скрипта.

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::paths::{self, data_dir};

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
    /// Пакет pacman (`/usr/bin/obs-legacy`): пакет з найновішого релізу, поставлений локальним
    /// файлом через `pacman -U`.
    Package,
}

/// Куди ставить програму пакет pacman (packaging/PKGBUILD).
pub const PACKAGE_EXE: &str = "/usr/bin/obs-legacy";

/// Як оновлювати саму програму з меню; None — лише вручну (зібрано без install.sh).
pub fn self_source() -> Option<SelfSource> {
    // Запущено копію з пакета: її оновлює лише новий пакет, навіть якщо десь лишився клон, з
    // якого колись ставив install.sh (його --update поставив би іншу копію в ~/.local/bin).
    if crate::service::exe_path() == Path::new(PACKAGE_EXE) {
        return Some(SelfSource::Package);
    }
    if let Some(dir) = source_dir() {
        return Some(SelfSource::Git(dir));
    }
    // install.sh з релізу пише сюди тег; запущено має бути саме ту копію, яку він поставив.
    let installed = fs::canonicalize(paths::home().join(".local/bin/obs-legacy"));
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
        crate::atomic::write(&path, SCRIPT.as_bytes(), 0o755)
            .with_context(|| trf!("не вдалося оновити {}", "couldn't update {}", path.display()))?;
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
