//! Версії: порівняння номерів, найновіші релізи на GitHub (`git ls-remote`, без завантаження коду)
//! і версії пакетів у репозиторіях pacman.

use std::env;
use std::process::{Command, Stdio};

pub const OBS_REPO: &str = "https://github.com/obsproject/obs-studio.git";

/// Ключ для порівняння версій: "v1.10.2" → [1, 10, 2].
pub fn version_key(s: &str) -> Vec<u64> {
    s.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Скільки чекати на відповідь GitHub (git ls-remote): без мережі запит не висить вічно.
const LS_REMOTE_TIMEOUT_SECS: &str = "20";

/// Які теги вважати релізами.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagStyle {
    /// OBS: рівно «X.Y.Z», без «v» і без суфіксів (-rc1, -beta).
    Obs,
    /// Плагіни й OBS-Legacy: «v?X.Y…» — щонайменше два числа, «v» на початку можна (так їх бере й скрипт).
    Release,
}

fn is_release_tag(tag: &str, style: TagStyle) -> bool {
    let t = match style {
        TagStyle::Obs => tag,
        TagStyle::Release => tag.trim_start_matches('v'),
    };
    let parts: Vec<&str> = t.split('.').collect();
    let count_ok = match style {
        TagStyle::Obs => parts.len() == 3,
        TagStyle::Release => parts.len() >= 2,
    };
    count_ok && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Найновіший тег-реліз репозиторію (git ls-remote, без завантаження коду).
pub fn latest_tag(repo: &str, style: TagStyle) -> Option<String> {
    let mut cmd = Command::new("timeout");
    cmd.args([LS_REMOTE_TIMEOUT_SECS, "git", "ls-remote", "--tags", "--refs", repo]).env("GIT_TERMINAL_PROMPT", "0");
    // Без запитань: служба у фоні не може ввести ні пароль, ні фразу ключа SSH.
    if env::var_os("GIT_SSH_COMMAND").is_none() {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    let out = cmd.stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split("refs/tags/").nth(1))
        .filter(|t| is_release_tag(t, style))
        .max_by_key(|t| version_key(t))
        .map(str::to_string)
}

/// Чи `latest` новіша за `current` ("v1.10.0" > "1.9.9"; "1.6.0" і "v1.6" — одна версія).
pub fn newer(latest: &str, current: &str) -> bool {
    let key = |s: &str| {
        let mut v = version_key(s);
        while v.len() > 1 && v.last() == Some(&0) {
            v.pop();
        }
        v
    };
    key(latest) > key(current)
}

#[derive(Debug, Clone, Default)]
pub struct Updates {
    pub obs_latest: Option<String>,
    /// (id плагіна, найновіший тег)
    pub plugins: Vec<(String, Option<String>)>,
    /// Найновіший реліз самого OBS-Legacy (тег, напр. "v0.0.2").
    pub self_latest: Option<String>,
    /// Версія ffmpeg у репозиторіях pacman (з локальної бази, без мережі): з неї
    /// зібрано ffmpeg-legacy-580xx, тож новіша означає «перезбери».
    pub ffmpeg_repo: Option<String>,
}

/// Версія пакета в репозиторіях pacman (`pacman -Si`, локальна база синхронізації).
pub fn repo_package_version(name: &str) -> Option<String> {
    let out = Command::new("pacman").args(["-Si", name]).env("LC_ALL", "C").stderr(Stdio::null()).output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("Version").and_then(|r| r.split_once(':')).map(|(_, v)| v.trim().to_string()))
        .filter(|v| !v.is_empty())
}

/// Чи `latest` новіша за `current` за правилами pacman (epoch:pkgver-pkgrel) —
/// утилітою vercmp з самого pacman.
pub fn pacman_newer(latest: &str, current: &str) -> bool {
    Command::new("vercmp")
        .args([latest, current])
        .stderr(Stdio::null())
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<i32>().ok())
        .is_some_and(|c| c > 0)
}

/// Найновіші OBS, плагіни (id, репозиторій) і OBS-Legacy (`self_repo`) — паралельно.
pub fn check_updates(plugins: &[(String, String)], self_repo: Option<&str>) -> Updates {
    let handles: Vec<_> = plugins
        .iter()
        .cloned()
        .map(|(id, repo)| std::thread::spawn(move || (id, latest_tag(&repo, TagStyle::Release))))
        .collect();
    let self_handle =
        self_repo.map(str::to_string).map(|r| std::thread::spawn(move || latest_tag(&r, TagStyle::Release)));
    let obs_latest = latest_tag(OBS_REPO, TagStyle::Obs);
    let plugins = handles.into_iter().filter_map(|h| h.join().ok()).collect();
    let self_latest = self_handle.and_then(|h| h.join().ok()).flatten();
    Updates { obs_latest, plugins, self_latest, ffmpeg_repo: repo_package_version("ffmpeg") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::headers_api;

    #[test]
    fn versions_and_tags() {
        assert!(version_key("33.0.0") > version_key("32.2.2"));
        assert!(version_key("v1.10.0") > version_key("1.9.9"));
        assert!(newer("v0.0.2", "0.0.1"));
        assert!(!newer("33.0.0", "33.0.0"));
        assert!(!newer("32.2.2", "33.0.0"));
        assert!(!newer("v1.6.0", "1.6"));
        assert!(newer("1.6.1", "v1.6"));
        assert!(is_release_tag("33.0.0", TagStyle::Obs));
        assert!(!is_release_tag("33.0.0-rc1", TagStyle::Obs));
        assert!(!is_release_tag("33.0", TagStyle::Obs));
        assert!(!is_release_tag("v33.0.0", TagStyle::Obs));
        assert!(is_release_tag("v1.6", TagStyle::Release));
        assert!(!is_release_tag("0.7.4.4-obs32.1.2", TagStyle::Release));
        assert_eq!(headers_api("n13.0.19.1").as_deref(), Some("13.0"));
        assert_eq!(headers_api("n12.2.72.0").as_deref(), Some("12.2"));
    }
}
