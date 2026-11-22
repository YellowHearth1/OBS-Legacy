//! Відомості для панелі стану: система, відеокарта й драйвер, встановлені
//! збірки OBS-Legacy та наявність оновлень. Лише читає — нічого не змінює.

use std::env;
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::builds::{BuildInfo, read_builds};
use crate::nvenc;
use crate::paths::{self, find_in_path};

pub const MARKER: &str = "Managed by build-obs-nvenc-legacy-artix";

/// Приписка пакетів OBS-Legacy: obs-studio-legacy-580xx, <плагін>-legacy-580xx, ffmpeg-legacy-580xx.
pub const PKG_SUFFIX: &str = "legacy-580xx";

pub const OBS_PKG: &str = "obs-studio-legacy-580xx";

pub const FFMPEG_PKG: &str = "ffmpeg-legacy-580xx";

/// Команда obs з пакета (у /usr, як в OBS з репозиторію).
pub const SYSTEM_OBS: &str = "/usr/bin/obs";

/// Стара схема (до пакетів): обгортка, хук у /etc і збірки в /opt. Скрипт
/// прибирає їх сам під час встановлення пакетів.
pub const WRAPPER: &str = "/usr/local/bin/obs";

pub const HOOK_FILE: &str = "/etc/pacman.d/hooks/90-obs-legacy.hook";

pub const LEGACY_LIBCHECK: &str = "/usr/local/lib/obs-legacy/check-libs";

/// Чи працює зараз скрипт збирання: він тримає блокування `.lock` у своєму кеші.
pub fn build_running() -> bool {
    let path = paths::cache_home().join("obs-studio-nvenc-artix/.lock");
    let Ok(file) = fs::File::open(path) else { return false };
    // SAFETY: flock на власному дескрипторі; блокування знімається разом із закриттям файлу.
    unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&file), libc::LOCK_SH | libc::LOCK_NB) != 0 }
}

/// Каталог збірок старої схеми (/opt/obs-studio-nvenc); для демо й тестів — `INSTALL_ROOT`.
pub fn install_root() -> PathBuf {
    env::var_os("INSTALL_ROOT")
        .filter(|v| !v.is_empty())
        .map_or_else(|| PathBuf::from("/opt/obs-studio-nvenc"), PathBuf::from)
}

/// Корінь, від якого читаються встановлені пакети (/usr/share/obs-legacy,
/// хук, check-libs). Для демо й тестів — OBS_LEGACY_ROOT; якщо задано лише
/// INSTALL_ROOT (так знімалися демо раніше), то він: демо не має бачити
/// справжній OBS користувача.
pub fn sys_root() -> PathBuf {
    ["OBS_LEGACY_ROOT", "INSTALL_ROOT"]
        .iter()
        .find_map(|var| env::var_os(var).filter(|v| !v.is_empty()))
        .map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

/// Відомості про встановлену збірку (усередині пакетів).
pub fn info_dir() -> PathBuf {
    sys_root().join("usr/share/obs-legacy")
}

/// pacman-хук із пакета OBS.
pub fn pkg_hook() -> PathBuf {
    sys_root().join("usr/share/libalpm/hooks/obs-legacy.hook")
}

/// Перевірка бібліотек, яку викликає pacman-хук: з пакета OBS, а для старої
/// схеми — з /usr/local/lib.
pub fn libcheck() -> PathBuf {
    let pkg = sys_root().join("usr/lib/obs-legacy/check-libs");
    if !pkg.exists() && sys_root() == Path::new("/") && Path::new(LEGACY_LIBCHECK).exists() {
        return PathBuf::from(LEGACY_LIBCHECK);
    }
    pkg
}

#[derive(Debug, Clone, Default)]
pub struct SysInfo {
    pub os: String,
    pub kernel: String,
    pub init: String,
    pub cpus: usize,
    pub mem_total_gib: f64,
    pub mem_avail_gib: f64,
    pub free_cache_gib: Option<f64>,
    /// Вільно на розділі з /usr (куди ставляться пакети).
    pub free_root_gib: Option<f64>,
    pub cpu_model: String,
    pub gpus: Vec<String>,
    pub driver: Option<String>,
    pub driver_pkg: Option<String>,
    pub nvenc_api: Option<String>,
    pub headers_api: Option<String>,
    /// Інший (не наш) пакет OBS, напр. obs-studio-git.
    pub obs_pkg: Option<String>,
    /// Встановлені пакети OBS-Legacy: «obs-studio-legacy-580xx 33.0.0-5».
    pub legacy_pkg: Option<String>,
    pub ffmpeg_pkg: Option<String>,
    /// Лишилося щось від старої схеми (/opt, обгортка) — прибере наступне встановлення.
    pub legacy_leftovers: bool,
    pub has_pacman: bool,
    pub is_root: bool,
    pub builds: Vec<BuildInfo>,
    pub wrapper_ours: bool,
    pub hook_ours: bool,
    pub obs_cmd: Option<PathBuf>,
}

impl SysInfo {
    pub fn active_build(&self) -> Option<&BuildInfo> {
        self.builds.iter().find(|b| b.active)
    }

    /// Чи команда obs відкриває саме OBS-Legacy: /usr/bin/obs з пакета або
    /// (стара схема) обгортка в /usr/local/bin.
    pub fn obs_cmd_ok(&self) -> bool {
        match (self.obs_cmd.as_deref(), self.active_build()) {
            (Some(p), Some(b)) if b.legacy => p.as_os_str() == WRAPPER && self.wrapper_ours,
            (Some(p), Some(_)) => p.as_os_str() == SYSTEM_OBS && self.legacy_pkg.is_some(),
            _ => false,
        }
    }

    /// Встановлену збірку оптимізовано під інший процесор (після заміни CPU
    /// такий OBS може аварійно завершуватися) — назва того процесора.
    pub fn built_for_other_cpu(&self) -> Option<String> {
        let b = self.active_build()?;
        let built = b.get("cpu_model").filter(|m| !m.is_empty())?;
        (b.get("cpu_tuning") == Some("native") && !self.cpu_model.is_empty() && built != self.cpu_model)
            .then(|| built.to_string())
    }

    pub fn driver_major(&self) -> Option<u32> {
        self.driver.as_deref()?.split('.').next()?.parse().ok()
    }

    /// Чи вистачає драйверу версії NVENC API для заголовків збірки.
    pub fn nvenc_compatible(&self) -> Option<bool> {
        let drv = nvenc::api_to_int(self.nvenc_api.as_deref()?)?;
        let hdr = nvenc::api_to_int(self.headers_api.as_deref()?)?;
        Some(drv >= hdr)
    }
}

/// Байтів у гібібайті.
const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
/// nvidia-smi інколи зависає (драйвер у поганому стані): панель стану не має чекати вічно.
const NVIDIA_SMI_TIMEOUT_SECS: &str = "10";

fn read_trim(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn cpu_model() -> String {
    fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|t| {
            t.lines().find_map(|l| {
                l.strip_prefix("model name").and_then(|r| r.split_once(':')).map(|(_, v)| v.trim().to_string())
            })
        })
        .unwrap_or_default()
}

fn os_name() -> String {
    fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=")).map(|v| v.trim_matches('"').to_string()))
        .unwrap_or_else(|| tr!("невідома ОС", "unknown OS").into())
}

fn meminfo() -> (f64, f64) {
    let text = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let get = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|v| v.split_whitespace().next()?.parse::<f64>().ok())
            .map_or(0.0, |kib| kib * 1024.0 / GIB)
    };
    (get("MemTotal:"), get("MemAvailable:"))
}

/// Вільне місце (ГіБ) на файловій системі, де лежить шлях або його найближчий предок.
pub fn free_gib(path: &Path) -> Option<f64> {
    let mut p = path.to_path_buf();
    while !p.exists() {
        p = p.parent()?.to_path_buf();
    }
    let c = CString::new(p.as_os_str().as_bytes()).ok()?;
    // SAFETY: statvfs заповнює передану структуру; шлях — коректний C-рядок.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    Some(st.f_bavail as f64 * st.f_frsize as f64 / GIB)
}

fn nvidia_from_proc() -> (Vec<String>, Option<String>) {
    let driver = read_trim("/proc/driver/nvidia/version").and_then(|t| {
        t.lines().next().and_then(|l| {
            l.split_whitespace()
                .find(|w| {
                    w.contains('.') && w.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
                })
                .map(str::to_string)
        })
    });
    let mut gpus: Vec<String> = fs::read_dir("/proc/driver/nvidia/gpus")
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter_map(|e| fs::read_to_string(e.path().join("information")).ok())
                .filter_map(|t| t.lines().find_map(|l| l.strip_prefix("Model:")).map(|m| m.trim().to_string()))
                .collect()
        })
        .unwrap_or_default();
    gpus.sort();
    (gpus, driver)
}

fn nvidia_from_smi() -> (Vec<String>, Option<String>) {
    let Ok(out) = Command::new("timeout")
        .args([NVIDIA_SMI_TIMEOUT_SECS, "nvidia-smi", "--query-gpu=name,driver_version", "--format=csv,noheader"])
        .stderr(Stdio::null())
        .output()
    else {
        return (Vec::new(), None);
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut gpus = Vec::new();
    let mut driver = None;
    for line in text.lines() {
        if let Some((name, ver)) = line.rsplit_once(',') {
            gpus.push(name.trim().to_string());
            driver.get_or_insert_with(|| ver.trim().to_string());
        }
    }
    (gpus, driver)
}

fn pacman_packages() -> Vec<(String, String)> {
    let Ok(out) = Command::new("pacman").arg("-Q").stderr(Stdio::null()).output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_once(' '))
        .map(|(n, v)| (n.to_string(), v.to_string()))
        .collect()
}

fn file_has_marker(path: &Path) -> bool {
    fs::read_to_string(path).is_ok_and(|t| t.contains(MARKER))
}

/// "n13.0.19.1" → "13.0"
pub fn headers_api(tag: &str) -> Option<String> {
    let v = tag.trim_start_matches('n');
    let mut parts = v.split('.');
    let (a, b) = (parts.next()?, parts.next()?);
    (a.parse::<u32>().is_ok() && b.parse::<u32>().is_ok()).then(|| format!("{a}.{b}"))
}

pub fn gather(exe: &Path, headers_tag: &str) -> SysInfo {
    let (mem_total_gib, mem_avail_gib) = meminfo();
    let (mut gpus, mut driver) = nvidia_from_proc();
    if gpus.is_empty() || driver.is_none() {
        let (g, d) = nvidia_from_smi();
        if gpus.is_empty() {
            gpus = g;
        }
        driver = driver.or(d);
    }
    let packages = pacman_packages();
    let find_pkg =
        |pred: &dyn Fn(&str) -> bool| packages.iter().find(|(n, _)| pred(n)).map(|(n, v)| format!("{n} {v}"));
    let (builds, _) = read_builds();
    let nvenc_api = nvenc::query(exe);
    // SAFETY: geteuid не має передумов.
    let is_root = unsafe { libc::geteuid() } == 0;
    let wrapper_ours = file_has_marker(Path::new(WRAPPER));
    SysInfo {
        os: os_name(),
        kernel: read_trim("/proc/sys/kernel/osrelease").unwrap_or_default(),
        init: read_trim("/proc/1/comm").unwrap_or_else(|| "?".into()),
        cpus: std::thread::available_parallelism().map_or(1, |n| n.get()),
        mem_total_gib,
        mem_avail_gib,
        free_cache_gib: free_gib(&paths::cache_home()),
        free_root_gib: free_gib(&sys_root().join("usr")),
        cpu_model: cpu_model(),
        gpus,
        driver,
        driver_pkg: find_pkg(&|n| {
            n.starts_with("nvidia") && n.ends_with("-utils") && !n.starts_with("nvidia-settings")
        }),
        // «auto» — заголовки під драйвер, тож вони завжди сумісні з ним.
        headers_api: if headers_tag == "auto" { nvenc_api.clone() } else { headers_api(headers_tag) },
        nvenc_api,
        obs_pkg: find_pkg(&|n| n == "obs-studio")
            .or_else(|| find_pkg(&|n| n.starts_with("obs-studio") && !n.ends_with(PKG_SUFFIX))),
        legacy_pkg: find_pkg(&|n| n == OBS_PKG),
        ffmpeg_pkg: find_pkg(&|n| n == FFMPEG_PKG),
        legacy_leftovers: wrapper_ours || install_root().join(".build-obs-nvenc-legacy-artix").exists(),
        has_pacman: find_in_path("pacman").is_some(),
        is_root,
        builds,
        wrapper_ours,
        hook_ours: file_has_marker(&pkg_hook()) || file_has_marker(Path::new(HOOK_FILE)),
        obs_cmd: find_in_path("obs"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obs_command_check() {
        let mut s = SysInfo {
            builds: vec![BuildInfo { active: true, ..Default::default() }],
            obs_cmd: Some(PathBuf::from(SYSTEM_OBS)),
            legacy_pkg: Some(format!("{OBS_PKG} 33.0.0-1")),
            ..Default::default()
        };
        assert!(s.obs_cmd_ok());
        // Обгортка старої схеми перекриває /usr/bin/obs.
        s.obs_cmd = Some(PathBuf::from(WRAPPER));
        assert!(!s.obs_cmd_ok());
        s.builds[0].legacy = true;
        s.wrapper_ours = true;
        assert!(s.obs_cmd_ok());
    }

    #[test]
    fn nvenc_compat() {
        let mut s = SysInfo { nvenc_api: Some("13.0".into()), headers_api: Some("13.0".into()), ..Default::default() };
        assert_eq!(s.nvenc_compatible(), Some(true));
        s.nvenc_api = Some("12.2".into());
        assert_eq!(s.nvenc_compatible(), Some(false));
        s.nvenc_api = None;
        assert_eq!(s.nvenc_compatible(), None);
    }
}
