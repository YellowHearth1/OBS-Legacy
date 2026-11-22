//! Що нового — для сповіщень на робочому столі: вийшли новіші OBS, плагіни чи
//! сам OBS-Legacy, або після оновлення системи власній збірці OBS забракло
//! бібліотек.
//!
//! `obs-legacy --check-updates` перевіряє один раз, `obs-legacy --watch` — у
//! фоні (так його запускає служба користувача, див. service/): нові версії —
//! раз на 12 годин, бібліотеки — після кожної транзакції pacman. Про кожну
//! новину сповіщає один раз: що вже показано, записано в
//! ~/.local/state/obs-legacy/updates.env (якщо файл видалити, про наявні
//! оновлення буде сповіщено ще раз).

mod auto;
mod notices;
mod prepare;
mod state;
mod watch;

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::builds::{self, BuildInfo};
use crate::catalog::{self, PluginInfo};
use crate::script;
use crate::service;
use crate::settings::Settings;
use crate::system;
use crate::versions::{self, Updates, newer};

pub use auto::notice_main as auto_update_notice;
pub use prepare::wait_script_free;
pub use state::State;
pub use watch::{check_main, log_path, running_pid, watch_main};

/// Репозиторій OBS-Legacy, якщо тека з вихідними кодами невідома.
pub const SELF_REPO: &str = "https://github.com/YellowHearth1/OBS-Legacy.git";

/// Релізи з готовою програмою: пакет obs-legacy, obs-legacy-x86_64 і install.sh до них.
pub const RELEASES: &str = "https://github.com/YellowHearth1/OBS-Legacy/releases";

/// install.sh з найновішого релізу: ставить готову програму з того самого релізу.
pub const INSTALLER_URL: &str = "https://github.com/YellowHearth1/OBS-Legacy/releases/latest/download/install.sh";

/// Куди ставить програму пакет pacman (packaging/PKGBUILD).
const PACKAGE_EXE: &str = "/usr/bin/obs-legacy";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginUpdate {
    pub id: String,
    pub name: String,
    pub installed: String,
    pub latest: String,
}

/// Як поставлено сам OBS-Legacy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SelfInstall {
    /// Оновлюється з меню («Оновити OBS-Legacy»): з теки з вихідними кодами чи з релізу.
    Menu,
    /// Пакет pacman (`/usr/bin/obs-legacy`): оновлення — новим пакетом з релізу.
    Package,
    /// Зібрано вручну: оновлювати так само — git pull і ./install.sh.
    #[default]
    Manual,
}

/// Що відомо про версії (результат перевірки через мережу).
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// OBS активної збірки (None — збірки немає).
    pub obs_installed: Option<String>,
    pub obs_latest: Option<String>,
    /// OBS_REF, якщо версію OBS зафіксовано (не «stable»): тоді про нові OBS не сповіщати.
    pub obs_pinned: Option<String>,
    /// Скільки плагінів збірки перевірено (зафіксовані PLUGIN_REF_<id> не рахуються).
    pub plugins_checked: usize,
    pub plugin_updates: Vec<PluginUpdate>,
    pub self_version: String,
    pub self_latest: Option<String>,
    /// Як поставлено сам OBS-Legacy — від цього залежить, як його оновлювати.
    pub self_install: SelfInstall,
    /// З якої версії ffmpeg з репозиторію зібрано ffmpeg-legacy-580xx (None — не встановлено).
    pub ffmpeg_built: Option<String>,
    /// Новіша версія ffmpeg у репозиторії — ffmpeg-legacy-580xx пора перезібрати.
    pub ffmpeg_new: Option<String>,
}

impl Facts {
    /// Чи була мережа (GitHub відповів про OBS).
    pub fn online(&self) -> bool {
        self.obs_latest.is_some()
    }

    pub fn obs_update(&self) -> Option<&str> {
        if self.obs_pinned.is_some() {
            return None;
        }
        let (installed, latest) = (self.obs_installed.as_deref()?, self.obs_latest.as_deref()?);
        newer(latest, installed).then_some(latest)
    }

    pub fn self_update(&self) -> Option<&str> {
        let latest = self.self_latest.as_deref()?;
        newer(latest, &self.self_version).then_some(latest)
    }

    pub fn summary(&self) -> String {
        if !self.online() {
            return tr!(
                "не вдалося перевірити: немає мережі чи GitHub недоступний",
                "couldn't check: no network or GitHub is unreachable"
            )
            .into();
        }
        let mut parts = Vec::new();
        if let Some(l) = self.obs_update() {
            parts.push(trf!("вийшов OBS {l}", "OBS {l} is out"));
        }
        if !self.plugin_updates.is_empty() {
            parts.push(trf!("нові версії плагінів: {}", "new plugin versions: {}", plugin_list(&self.plugin_updates)));
        }
        if let Some(v) = &self.ffmpeg_new {
            parts.push(trf!("у репозиторії новий ffmpeg {v}", "new ffmpeg {v} in the repository"));
        }
        if let Some(l) = self.self_update() {
            parts.push(trf!("вийшов OBS-Legacy {}", "OBS-Legacy {} is out", l.trim_start_matches('v')));
        }
        if parts.is_empty() { tr!("нового немає", "nothing new").into() } else { parts.join("; ") }
    }
}

pub fn plugin_list(list: &[PluginUpdate]) -> String {
    list.iter().map(|p| format!("{} {}", p.name, p.latest)).collect::<Vec<_>>().join(", ")
}

/// Значення налаштування: змінна середовища має пріоритет, як і в скрипті.
pub(crate) fn setting(settings: &Settings, key: &str) -> String {
    Settings::env_override(key).filter(|v| !v.is_empty()).or_else(|| settings.raw(key)).unwrap_or_default()
}

/// Звідки перевіряти нові версії OBS-Legacy: origin теки з вихідними кодами
/// (з неї ж оновлює пункт «Оновити OBS-Legacy») або офіційний репозиторій.
pub fn self_repo() -> String {
    script::source_dir()
        .and_then(|dir| {
            let out = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["remote", "get-url", "origin"])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .ok()?;
            out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        })
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| SELF_REPO.to_string())
}

/// Плагіни активної збірки, за якими стежимо: (id, назва, встановлена версія,
/// репозиторій). Зафіксовані (PLUGIN_REF_<id>) не оновлюються — їх пропускаємо.
fn tracked_plugins(
    active: Option<&BuildInfo>,
    settings: &Settings,
    catalog: &[PluginInfo],
) -> Vec<(String, String, String, String)> {
    active
        .map(BuildInfo::plugins)
        .unwrap_or_default()
        .into_iter()
        .filter(|(id, _)| setting(settings, &format!("PLUGIN_REF_{}", id.replace('-', "_"))).is_empty())
        .filter_map(|(id, ver)| {
            let p = catalog.iter().find(|p| p.id == id)?;
            Some((id, p.name.clone(), ver, p.repo.clone()))
        })
        .collect()
}

/// Факти з відповіді GitHub (`u`) і стану збірки. Те саме бачить і TUI.
pub fn facts_from(
    active: Option<&BuildInfo>,
    u: &Updates,
    settings: &Settings,
    catalog: &[PluginInfo],
    self_menu: bool,
) -> Facts {
    let obs_ref = setting(settings, "OBS_REF");
    let tracked = tracked_plugins(active, settings, catalog);
    let plugin_updates = tracked
        .iter()
        .filter_map(|(id, name, installed, _)| {
            let latest = u.plugins.iter().find(|(pid, _)| pid == id)?.1.clone()?;
            newer(&latest, installed).then(|| PluginUpdate {
                id: id.clone(),
                name: name.clone(),
                installed: installed.clone(),
                latest,
            })
        })
        .collect();
    // ffmpeg-legacy-580xx зібрано зі старішої версії ffmpeg, ніж тепер у репозиторії (лише версія,
    // без pkgrel: перезбирання під нові бібліотеки ловлять keep-libs і check-libs).
    // Коли збирання ffmpeg вимкнено, «install» його й не перезбере — тоді мовчимо.
    let ffmpeg_built = active.and_then(|b| b.get("ffmpeg.arch_version")).filter(|v| !v.is_empty()).map(str::to_string);
    let ffmpeg_new = match (&ffmpeg_built, &u.ffmpeg_repo) {
        (Some(built), Some(repo))
            if setting(settings, "BUILD_FFMPEG") != "0"
                && versions::pacman_newer(versions::without_pkgrel(repo), versions::without_pkgrel(built)) =>
        {
            Some(repo.clone())
        }
        _ => None,
    };
    Facts {
        obs_installed: active.and_then(|b| b.get("obs_version")).map(str::to_string),
        obs_latest: u.obs_latest.clone(),
        obs_pinned: (!obs_ref.is_empty() && obs_ref != "stable").then_some(obs_ref),
        plugins_checked: tracked.len(),
        plugin_updates,
        self_version: env!("CARGO_PKG_VERSION").to_string(),
        self_latest: u.self_latest.clone(),
        self_install: if self_menu {
            SelfInstall::Menu
        } else if service::exe_path() == Path::new(PACKAGE_EXE) {
            SelfInstall::Package
        } else {
            SelfInstall::Manual
        },
        ffmpeg_built,
        ffmpeg_new,
    }
}

/// Новий ffmpeg у репозиторії (лише локальна база pacman, без мережі) — для перевірки після
/// транзакції pacman: `pacman -Sy` користувача міг принести нову версію.
pub fn ffmpeg_new_local() -> Option<String> {
    let (builds, _) = builds::read_builds();
    let active = builds.iter().find(|b| b.active);
    let u = Updates { ffmpeg_repo: versions::repo_package_version("ffmpeg"), ..Default::default() };
    facts_from(active, &u, &Settings::load(), &[], false).ffmpeg_new
}

/// Перевірити нові версії (мережа: git ls-remote до GitHub).
pub fn gather() -> Facts {
    gather_with_updates().0
}

/// Те саме, що `gather`, і ще відповідь GitHub — щоб потім перераховувати факти без мережі.
pub fn gather_with_updates() -> (Facts, Updates) {
    let (builds, _) = builds::read_builds();
    let active = builds.iter().find(|b| b.active);
    let settings = Settings::load();
    let catalog = script::ensure_extracted().ok().and_then(|s| catalog::load(&s).ok()).unwrap_or_default();
    let repos: Vec<(String, String)> =
        tracked_plugins(active, &settings, &catalog).into_iter().map(|(id, _, _, repo)| (id, repo)).collect();
    let u = versions::check_updates(&repos, Some(&self_repo()));
    (facts_from(active, &u, &settings, &catalog, script::self_source().is_some()), u)
}

/// Факти з уже відомих версій (без мережі): після транзакції pacman встановлене могло змінитися
/// (поставили новий набір, `pacman -Sy` приніс новий ffmpeg).
pub fn facts_offline(u: &Updates) -> Facts {
    let (builds, _) = builds::read_builds();
    let active = builds.iter().find(|b| b.active);
    let catalog = script::ensure_extracted().ok().and_then(|s| catalog::load(&s).ok()).unwrap_or_default();
    let u = Updates { ffmpeg_repo: versions::repo_package_version("ffmpeg"), ..u.clone() };
    facts_from(active, &u, &Settings::load(), &catalog, script::self_source().is_some())
}

/// Список бібліотек, які зберіг хук keep-libs (скрипт, KEPT_LIBS_LIST): у системі їх уже немає, а
/// пакети OBS-Legacy ще працюють на копіях — їх треба перезібрати під нові.
const KEPT_LIBS_LIST: &str = "/var/lib/obs-legacy/kept-libs.tsv";

/// «libjxl.so.0.12 (ffmpeg-legacy-580xx)» на кожну збережену бібліотеку, потрібну пакетам набору;
/// порожньо — нічого.
pub fn kept_libs() -> Vec<String> {
    let Ok(text) = fs::read_to_string(KEPT_LIBS_LIST) else {
        return Vec::new();
    };
    parse_kept(&text, &set_packages())
}

/// Пакети встановленого набору (OBS, плагіни, ffmpeg) — власники його файлів у /usr/share/obs-legacy,
/// як `set_packages` у скрипті. Лише їх набір перезбирає; інший пакет з тією самою припискою (чужий чи
/// забутий) — ні, тож його збережені бібліотеки не привід перезбирати.
fn set_packages() -> Vec<String> {
    let dir = system::info_dir();
    let mut files = vec![dir.join("BUILDINFO.txt"), dir.join("ffmpeg.txt")];
    if let Ok(entries) = fs::read_dir(dir.join("plugins")) {
        files.extend(entries.flatten().map(|e| e.path()));
    }
    files.retain(|f| f.is_file());
    if files.is_empty() {
        return Vec::new();
    }
    let out =
        Command::new("pacman").arg("-Qqo").arg("--").args(&files).env("LC_ALL", "C").stderr(Stdio::null()).output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().map(str::to_string).collect()).unwrap_or_default()
}

/// Рядки списку keep-libs: «бібліотека<TAB>пакети через пробіл» — лише ті, що потрібні пакетам `set`
/// (рядок без пакетів лишається: невідомо чий — краще показати).
pub fn parse_kept(text: &str, set: &[String]) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let (so, by) = l.split_once('\t').unwrap_or((l, ""));
            let (so, by) = (so.trim(), by.trim());
            let ours = by.is_empty() || by.split_whitespace().any(|p| set.iter().any(|s| s == p));
            (!so.is_empty() && ours).then(|| if by.is_empty() { so.to_string() } else { format!("{so} ({by})") })
        })
        .collect()
}

/// check-libs проганяє ldd по всіх файлах пакетів OBS-Legacy: на повільному диску це хвилини, але не вічність.
const LIBCHECK_TIMEOUT_SECS: &str = "300";

/// Чого бракує власній збірці OBS — тією ж перевіркою, що й pacman-хук.
/// None — перевірки немає (хук не встановлено чи вимкнено).
pub fn check_libs() -> Option<Vec<String>> {
    let libcheck = system::libcheck();
    let text = fs::read_to_string(&libcheck).ok()?;
    if !text.contains(system::MARKER) {
        return None;
    }
    let out = Command::new("timeout")
        .args([LIBCHECK_TIMEOUT_SECS, "bash"])
        .arg(&libcheck)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Some(parse_libcheck(&String::from_utf8_lossy(&out.stdout)))
}

/// Рядки проблем з виводу перевірки бібліотек (pacman-хука).
pub fn parse_libcheck(out: &str) -> Vec<String> {
    out.lines()
        .filter(|l| l.starts_with("      "))
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('…'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_list_lines() {
        let text = "libjxl.so.0.12\tffmpeg-legacy-580xx\nlibx264.so.165\tobs-studio-legacy-580xx ffmpeg-legacy-580xx\n\
                    \nbare.so.1\nlibolt.so.1\tolt-probe-legacy-580xx\n";
        let set: Vec<String> = ["obs-studio-legacy-580xx", "ffmpeg-legacy-580xx"].map(String::from).into();
        assert_eq!(
            parse_kept(text, &set),
            [
                "libjxl.so.0.12 (ffmpeg-legacy-580xx)",
                "libx264.so.165 (obs-studio-legacy-580xx ffmpeg-legacy-580xx)",
                "bare.so.1"
            ]
        );
        assert!(parse_kept("", &set).is_empty());
        // Пакет не з набору (чужий чи забутий плагін): набір його не перезбере — не привід.
        assert!(parse_kept("libolt.so.1\tolt-probe-legacy-580xx\n", &set).is_empty());
    }
}
