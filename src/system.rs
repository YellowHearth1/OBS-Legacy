//! Відомості для панелі стану: система, відеокарта й драйвер, встановлені
//! збірки OBS-Legacy та наявність оновлень. Лише читає — нічого не змінює.

use std::collections::BTreeMap;
use std::env;
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::nvenc;
use crate::script;

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
pub const OBS_REPO: &str = "https://github.com/obsproject/obs-studio.git";

/// Каталог збірок старої схеми (/opt/obs-studio-nvenc).
/// Чи працює зараз скрипт збирання: він тримає блокування `.lock` у своєму кеші.
pub fn build_running() -> bool {
    let path = script::cache_home().join("obs-studio-nvenc-artix/.lock");
    let Ok(file) = fs::File::open(path) else { return false };
    // SAFETY: flock на власному дескрипторі; блокування знімається разом із закриттям файлу.
    unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&file), libc::LOCK_SH | libc::LOCK_NB) != 0 }
}

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
pub struct BuildInfo {
    pub label: String,
    pub fields: BTreeMap<String, String>,
    pub complete: bool,
    pub active: bool,
    /// Збірка старої схеми (/opt), а не набір пакетів.
    pub legacy: bool,
}

impl BuildInfo {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }

    pub fn obs_version(&self) -> &str {
        self.get("obs_version").unwrap_or("?")
    }

    /// Дата збірки у вигляді «2026-10-28 14:05».
    pub fn built(&self) -> String {
        self.get("built").map(|b| b.replace('T', " ").chars().take(16).collect()).unwrap_or_default()
    }

    /// (id, версія) плагінів у цій збірці.
    pub fn plugins(&self) -> Vec<(String, String)> {
        self.fields
            .iter()
            .filter_map(|(k, v)| {
                let id = k.strip_prefix("plugin.")?;
                Some((id.to_string(), v.split_whitespace().next().unwrap_or("").to_string()))
            })
            .collect()
    }

    pub fn plugins_failed(&self) -> Vec<String> {
        self.get("plugins_failed").map(|s| s.split_whitespace().map(str::to_string).collect()).unwrap_or_default()
    }

    /// Під що оптимізовано: «ivybridge · -O2 · CUDA sm_61» (для старих збірок — None).
    pub fn profile(&self) -> Option<String> {
        let tuning = self.get("cpu_tuning").filter(|v| !v.is_empty())?;
        let cpu = match (tuning, self.get("cpu_march").filter(|v| !v.is_empty())) {
            ("native", Some(march)) => march.to_string(),
            (t, _) => t.to_string(),
        };
        let mut s = format!("{cpu} · -O{}", self.get("opt_level").unwrap_or("2"));
        if let Some(cc) = self.get("gpu_cc").filter(|v| !v.is_empty()) {
            s.push_str(&format!(" · CUDA sm_{}", cc.replace('.', "")));
        }
        Some(s)
    }
}

fn parse_buildinfo(text: &str) -> BTreeMap<String, String> {
    text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect()
}

/// Відомості встановленої збірки: BUILDINFO пакета OBS, версії плагінів з їхніх
/// пакетів і ffmpeg (ключі `ffmpeg.*`). None — пакети OBS-Legacy не встановлено.
fn installed_info_from(dir: &Path) -> Option<BTreeMap<String, String>> {
    let mut fields = parse_buildinfo(&fs::read_to_string(dir.join("BUILDINFO.txt")).ok()?);
    if let Ok(rd) = fs::read_dir(dir.join("plugins")) {
        for e in rd.filter_map(Result::ok) {
            if let Ok(t) = fs::read_to_string(e.path()) {
                fields.extend(parse_buildinfo(&t));
            }
        }
    }
    if let Ok(t) = fs::read_to_string(dir.join("ffmpeg.txt")) {
        fields.extend(parse_buildinfo(&t).into_iter().map(|(k, v)| (format!("ffmpeg.{k}"), v)));
    }
    Some(fields)
}

/// Підкаталоги з BUILDINFO.txt і позначкою .complete (набори пакетів чи збірки в /opt).
fn read_build_dirs(dir: &Path, active: Option<&str>, legacy: bool) -> Vec<BuildInfo> {
    fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| {
                    let label = e.file_name().to_string_lossy().into_owned();
                    let dir = e.path();
                    BuildInfo {
                        fields: fs::read_to_string(dir.join("BUILDINFO.txt"))
                            .map(|t| parse_buildinfo(&t))
                            .unwrap_or_default(),
                        complete: dir.join(".complete").exists(),
                        active: active == Some(label.as_str()),
                        label,
                        legacy,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Збірки для панелі стану й відкату: набори пакетів (активний — встановлений),
/// а поки пакетів немає — ще й збірки старої схеми в /opt.
pub fn read_builds() -> (Vec<BuildInfo>, Option<String>) {
    read_builds_from(&info_dir(), &script::package_store(), &install_root())
}

fn read_builds_from(info: &Path, store: &Path, legacy_root: &Path) -> (Vec<BuildInfo>, Option<String>) {
    let installed = installed_info_from(info);
    let mut active = installed.as_ref().and_then(|f| f.get("label").cloned());
    let mut builds = read_build_dirs(store, active.as_deref(), false);
    if let (Some(fields), Some(label)) = (installed, active.clone()) {
        match builds.iter_mut().find(|b| b.label == label) {
            // Для встановленого набору — відомості з системи: плагін могли прибрати окремо.
            Some(b) => b.fields = fields,
            None => builds.push(BuildInfo { label, fields, complete: true, active: true, legacy: false }),
        }
    }
    if active.is_none() {
        active = fs::read_link(legacy_root.join("current"))
            .ok()
            .and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned()));
        builds.extend(read_build_dirs(&legacy_root.join("versions"), active.as_deref(), true));
    }
    builds.sort_by(|a, b| b.label.cmp(&a.label));
    (builds, active)
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

pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path).map(|d| d.join(name)).find(|p| {
        p.metadata()
            .map(|m| m.is_file() && std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o111 != 0)
            .unwrap_or(false)
    })
}

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
            .map_or(0.0, |kib| kib / 1024.0 / 1024.0)
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
    Some(st.f_bavail as f64 * st.f_frsize as f64 / 1024.0 / 1024.0 / 1024.0)
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
        .args(["10", "nvidia-smi", "--query-gpu=name,driver_version", "--format=csv,noheader"])
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
        free_cache_gib: free_gib(&script::cache_home()),
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

/// Ключ для порівняння версій: "v1.10.2" → [1, 10, 2].
pub fn version_key(s: &str) -> Vec<u64> {
    s.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn is_release_tag(tag: &str, three_parts: bool) -> bool {
    let t = if three_parts { tag } else { tag.trim_start_matches('v') };
    let parts: Vec<&str> = t.split('.').collect();
    let n_ok = if three_parts { parts.len() == 3 } else { parts.len() >= 2 };
    n_ok && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Найновіший тег-реліз репозиторію (git ls-remote, без завантаження коду).
pub fn latest_tag(repo: &str, three_parts: bool) -> Option<String> {
    let mut cmd = Command::new("timeout");
    cmd.args(["20", "git", "ls-remote", "--tags", "--refs", repo]).env("GIT_TERMINAL_PROMPT", "0");
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
        .filter(|t| is_release_tag(t, three_parts))
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
    let handles: Vec<_> =
        plugins.iter().cloned().map(|(id, repo)| std::thread::spawn(move || (id, latest_tag(&repo, false)))).collect();
    let self_handle = self_repo.map(str::to_string).map(|r| std::thread::spawn(move || latest_tag(&r, false)));
    let obs_latest = latest_tag(OBS_REPO, true);
    let plugins = handles.into_iter().filter_map(|h| h.join().ok()).collect();
    let self_latest = self_handle.and_then(|h| h.join().ok()).flatten();
    Updates { obs_latest, plugins, self_latest, ffmpeg_repo: repo_package_version("ffmpeg") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buildinfo_parsing() {
        let b = BuildInfo {
            fields: parse_buildinfo(
                "label=x\nobs_version=33.0.0\nbuilt=2026-10-28T14:05:11+02:00\nplugin.obs-multi-rtmp=0.8.0.0 abc123\nplugins_failed=waveform\n",
            ),
            ..Default::default()
        };
        assert_eq!(b.obs_version(), "33.0.0");
        assert_eq!(b.built(), "2026-10-28 14:05");
        assert_eq!(b.plugins(), [("obs-multi-rtmp".to_string(), "0.8.0.0".to_string())]);
        assert_eq!(b.plugins_failed(), ["waveform"]);
    }

    #[test]
    fn versions_and_tags() {
        assert!(version_key("33.0.0") > version_key("32.2.2"));
        assert!(version_key("v1.10.0") > version_key("1.9.9"));
        assert!(newer("v0.0.2", "0.0.1"));
        assert!(!newer("33.0.0", "33.0.0"));
        assert!(!newer("32.2.2", "33.0.0"));
        assert!(!newer("v1.6.0", "1.6"));
        assert!(newer("1.6.1", "v1.6"));
        assert!(is_release_tag("33.0.0", true));
        assert!(!is_release_tag("33.0.0-rc1", true));
        assert!(!is_release_tag("33.0", true));
        assert!(is_release_tag("v1.6", false));
        assert!(!is_release_tag("0.7.4.4-obs32.1.2", false));
        assert_eq!(headers_api("n13.0.19.1").as_deref(), Some("13.0"));
        assert_eq!(headers_api("n12.2.72.0").as_deref(), Some("12.2"));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("obs-legacy-sys-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn packages_and_legacy_builds() {
        let root = temp_dir("builds");
        let (info, store, legacy) = (root.join("info"), root.join("store"), root.join("opt"));
        write(&legacy.join("versions/20261029-093154_33.0.0/BUILDINFO.txt"), "obs_version=33.0.0\n");
        write(&legacy.join("versions/20261029-093154_33.0.0/.complete"), "");
        std::os::unix::fs::symlink("versions/20261029-093154_33.0.0", legacy.join("current")).unwrap();
        write(
            &store.join("20261029-180000_33.0.0/BUILDINFO.txt"),
            "label=20261029-180000_33.0.0\nobs_version=33.0.0\n",
        );
        write(&store.join("20261029-180000_33.0.0/.complete"), "");

        // Пакетів ще немає: набір зібрано, але активна — стара збірка в /opt.
        let (builds, active) = read_builds_from(&info, &store, &legacy);
        assert_eq!(active.as_deref(), Some("20261029-093154_33.0.0"));
        assert_eq!(builds.len(), 2);
        assert!(builds.iter().any(|b| b.legacy && b.active));
        assert!(builds.iter().any(|b| !b.legacy && !b.active && b.complete));

        // Пакети встановлено: активний — набір, плагіни й ffmpeg — з відомостей у системі, /opt не видно.
        write(&info.join("BUILDINFO.txt"), "label=20261029-180000_33.0.0\nobs_version=33.0.0\ncpu_tuning=native\n");
        write(&info.join("plugins/obs-multi-rtmp"), "plugin.obs-multi-rtmp=0.8.0.0 f23d759786b7\n");
        write(&info.join("ffmpeg.txt"), "arch_version=2:9.0.2-2\n");
        let (builds, active) = read_builds_from(&info, &store, &legacy);
        assert_eq!(active.as_deref(), Some("20261029-180000_33.0.0"));
        assert_eq!(builds.len(), 1);
        let b = &builds[0];
        assert!(b.active && !b.legacy);
        assert_eq!(b.plugins(), [("obs-multi-rtmp".to_string(), "0.8.0.0".to_string())]);
        assert_eq!(b.get("ffmpeg.arch_version"), Some("2:9.0.2-2"));

        // Набір зі сховища видалили — встановлена збірка однаково видна.
        fs::remove_dir_all(&store).unwrap();
        let (builds, _) = read_builds_from(&info, &store, &legacy);
        assert!(builds.len() == 1 && builds[0].active && builds[0].complete);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn profile_and_cpu_change() {
        let b = BuildInfo {
            fields: parse_buildinfo(
                "cpu_tuning=native\ncpu_march=ivybridge\nopt_level=2\ngpu_cc=6.1\ncpu_model=Intel Xeon E3-1275 V2\n",
            ),
            active: true,
            complete: true,
            ..Default::default()
        };
        assert_eq!(b.profile().as_deref(), Some("ivybridge · -O2 · CUDA sm_61"));
        let mut s = SysInfo { builds: vec![b], cpu_model: "Intel Xeon E3-1275 V2".into(), ..Default::default() };
        assert_eq!(s.built_for_other_cpu(), None);
        s.cpu_model = "AMD Ryzen 5 5600".into();
        assert_eq!(s.built_for_other_cpu().as_deref(), Some("Intel Xeon E3-1275 V2"));
        // Переносна збірка (generic) працює на будь-якому процесорі.
        s.builds[0].fields.insert("cpu_tuning".into(), "generic".into());
        assert_eq!(s.built_for_other_cpu(), None);
        assert_eq!(s.builds[0].profile().as_deref(), Some("generic · -O2 · CUDA sm_61"));
        assert_eq!(BuildInfo::default().profile(), None);
    }

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
