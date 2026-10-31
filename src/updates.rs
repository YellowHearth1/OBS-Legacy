//! Що нового — для сповіщень на робочому столі: вийшли новіші OBS, плагіни чи
//! сам OBS-Legacy, або після оновлення системи власній збірці OBS забракло
//! бібліотек.
//!
//! `obs-legacy --check-updates` перевіряє один раз, `obs-legacy --watch` — у
//! фоні (так його запускає служба користувача, див. service.rs): нові версії —
//! раз на 12 годин, бібліотеки — після кожної транзакції pacman. Про кожну
//! новину сповіщає один раз: що вже показано, записано в
//! ~/.local/state/obs-legacy/updates.env (якщо файл видалити, про наявні
//! оновлення буде сповіщено ще раз).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::catalog::{self, PluginInfo};
use crate::cli;
use crate::notify;
use crate::script;
use crate::service;
use crate::settings::Settings;
use crate::system::{self, BuildInfo, Updates, newer};

/// Репозиторій OBS-Legacy, якщо тека з вихідними кодами невідома.
pub const SELF_REPO: &str = "https://github.com/YellowHearth1/OBS-Legacy.git";
/// Релізи з готовою програмою: пакет obs-legacy, obs-legacy-x86_64 і install.sh до них.
pub const RELEASES: &str = "https://github.com/YellowHearth1/OBS-Legacy/releases";
/// install.sh з найновішого релізу: ставить готову програму з того самого релізу.
pub const INSTALLER_URL: &str = "https://github.com/YellowHearth1/OBS-Legacy/releases/latest/download/install.sh";
/// Куди ставить програму пакет pacman (packaging/PKGBUILD).
const PACKAGE_EXE: &str = "/usr/bin/obs-legacy";
const PACMAN_DB: &str = "/var/lib/pacman/local";
const PACMAN_LOCK: &str = "/var/lib/pacman/db.lck";

/// Як часто перевіряти нові версії (секунди).
pub const PERIOD: u64 = 12 * 3600;
/// Повторна спроба, якщо мережі не було.
const RETRY: u64 = 30 * 60;
/// Перша перевірка після запуску: дати сесії й мережі піднятися.
const FIRST_DELAY: u64 = 2 * 60;
/// Як часто прокидатися (бібліотеки після pacman, сповіщення, що чекають сесії).
const TICK: u64 = 5 * 60;

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Місцевий час «2026-10-29 14:05».
pub fn local_time(secs: u64) -> String {
    static TZ: Once = Once::new();
    unsafe extern "C" {
        fn tzset();
    }
    // SAFETY: tzset лише читає TZ і /etc/localtime; виклик один раз до localtime_r.
    TZ.call_once(|| unsafe { tzset() });
    let t = secs as libc::time_t;
    // SAFETY: localtime_r заповнює передану структуру й не зберігає вказівників.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return secs.to_string();
    }
    format!("{:04}-{:02}-{:02} {:02}:{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min)
}

// ------------------------------------------------------------------ стан

/// ~/.local/state/obs-legacy/updates.env: KEY=VALUE.
#[derive(Debug, Default, Clone)]
pub struct State {
    map: BTreeMap<String, String>,
}

impl State {
    pub fn path() -> PathBuf {
        script::state_dir().join("updates.env")
    }

    pub fn load() -> Self {
        Self::parse(&fs::read_to_string(Self::path()).unwrap_or_default())
    }

    fn parse(text: &str) -> Self {
        let map = text
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect();
        Self { map }
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::path();
        fs::create_dir_all(script::state_dir())?;
        let mut body = String::from(tr!(
            "# OBS-Legacy: стан фонової перевірки оновлень. Якщо файл видалити, про наявні\n\
             # оновлення буде сповіщено ще раз.\n",
            "# OBS-Legacy: state of the background update check. Delete this file to be notified\n\
             # about the available updates again.\n"
        ));
        for (k, v) in &self.map {
            let _ = writeln!(body, "{k}={v}");
        }
        let tmp = path.with_extension(format!("env.tmp{}", std::process::id()));
        fs::write(&tmp, body)?;
        fs::rename(&tmp, &path)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(String::as_str)
    }

    fn num(&self, key: &str) -> Option<u64> {
        self.get(key)?.parse().ok()
    }

    fn set(&mut self, key: &str, value: impl ToString) {
        self.map.insert(key.to_string(), value.to_string());
    }

    fn remove(&mut self, key: &str) {
        self.map.remove(key);
    }

    /// Коли востаннє перевіряли нові версії (успішно чи ні).
    pub fn last_check(&self) -> Option<u64> {
        self.num("checked")
    }

    /// Що знайшла остання перевірка — коротко.
    pub fn summary(&self) -> Option<&str> {
        self.get("summary")
    }

    /// Коли перевіряти наступного разу: через 12 год після вдалої перевірки,
    /// через 30 хв після невдалої, але не раніше ніж за 2 хв після запуску.
    pub fn next_check(&self, now: u64) -> u64 {
        let due = match (self.last_check(), self.num("ok")) {
            (Some(t), Some(ok)) if ok == t => t + PERIOD,
            (Some(t), _) => t + RETRY,
            (None, _) => 0,
        };
        due.max(now + FIRST_DELAY)
    }
}

// --------------------------------------------------------------- новини

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginUpdate {
    pub id: String,
    pub name: String,
    pub installed: String,
    pub latest: String,
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
    /// OBS-Legacy можна оновити з меню (відома тека з вихідними кодами).
    pub self_menu: bool,
    /// OBS-Legacy поставлено пакетом pacman (`/usr/bin/obs-legacy`): оновлення — новим пакетом з релізу.
    pub self_package: bool,
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

/// Одне сповіщення.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// Про що саме: (ключ, значення) — версія або відбиток проблеми.
    pub items: Vec<(String, String)>,
    pub title: String,
    pub body: String,
    pub urgent: bool,
}

impl Notice {
    /// Чи є в ньому щось, про що ще не сповіщали.
    pub fn is_new(&self, st: &State) -> bool {
        self.items.iter().any(|(k, v)| st.get(&format!("notified.{k}")) != Some(v.as_str()))
    }

    pub fn mark(&self, st: &mut State) {
        for (k, v) in &self.items {
            st.set(&format!("notified.{k}"), v);
        }
    }

    pub fn message<'a>(&'a self, open: Option<&'a Path>) -> notify::Message<'a> {
        notify::Message { title: &self.title, body: &self.body, urgent: self.urgent, open }
    }
}

/// Як оновити OBS (і плагіни, і ffmpeg) — пункт меню.
fn how_install() -> &'static str {
    tr!("OBS-Legacy → «Встановити / оновити OBS»", "OBS-Legacy → “Install / update OBS”")
}

/// Сповіщення про нові версії (без бібліотек).
pub fn notices(f: &Facts) -> Vec<Notice> {
    let mut out = Vec::new();
    let plugin_items = || f.plugin_updates.iter().map(|p| (format!("plugin.{}", p.id), p.latest.clone()));
    if let (Some(latest), Some(installed)) = (f.obs_update(), f.obs_installed.as_deref()) {
        let how = how_install();
        let mut body = trf!(
            "У тебе OBS {installed}. Онови: {how}. Нова збірка ввімкнеться лише після перевірки NVENC, \
             а поточна лишиться для відкату.",
            "You have OBS {installed}. Update: {how}. The new build is switched on only after the NVENC check, \
             and the current one stays for rollback."
        );
        if !f.plugin_updates.is_empty() {
            let list = plugin_list(&f.plugin_updates);
            body.push_str(&trf!(
                " Разом з OBS оновляться плагіни: {list}.",
                " Plugins are updated together with OBS: {list}."
            ));
        }
        let mut items = vec![("obs".to_string(), latest.to_string())];
        items.extend(plugin_items());
        out.push(Notice {
            items, title: trf!("Вийшов OBS {latest}", "OBS {latest} is out"), body, urgent: false
        });
    } else if !f.plugin_updates.is_empty() {
        let (title, what) = match f.plugin_updates.as_slice() {
            [p] => (
                trf!("Нова версія плагіна {}: {}", "New version of the {} plugin: {}", p.name, p.latest),
                trf!("У тебе {}", "You have {}", p.installed),
            ),
            many => {
                (trf!("Нові версії плагінів OBS: {}", "New OBS plugin versions: {}", many.len()), plugin_list(many))
            }
        };
        let how = how_install();
        out.push(Notice {
            items: plugin_items().collect(),
            title,
            body: trf!(
                "{what}. Плагіни оновлюються разом з OBS: {how}.",
                "{what}. Plugins are updated together with OBS: {how}."
            ),
            urgent: false,
        });
    }
    if let Some(new) = &f.ffmpeg_new {
        let built = f.ffmpeg_built.as_deref().unwrap_or(tr!("старішої версії", "an older version"));
        let how = how_install();
        out.push(Notice {
            items: vec![("ffmpeg".into(), new.clone())],
            title: trf!(
                "Новий ffmpeg {new} — перезбери ffmpeg-legacy-580xx",
                "New ffmpeg {new} — rebuild ffmpeg-legacy-580xx"
            ),
            body: trf!(
                "Твій ffmpeg-legacy-580xx зібрано з {built}. Перезбери його з NVENC під свій драйвер: {how} \
                 (ffmpeg збирається ~15 хв). До того працює поточний.",
                "Your ffmpeg-legacy-580xx was built from {built}. Rebuild it with NVENC for your driver: {how} \
                 (ffmpeg takes ~15 min to build). Until then the current one works."
            ),
            urgent: false,
        });
    }
    if let Some(latest) = f.self_update() {
        let file = format!("obs-legacy-{}-1-x86_64.pkg.tar.zst", latest.trim_start_matches('v'));
        let how = if f.self_menu {
            tr!(
                "Онови: OBS-Legacy → Додатково → «Оновити OBS-Legacy».",
                "Update: OBS-Legacy → More → “Update OBS-Legacy”."
            )
            .to_string()
        } else if f.self_package {
            trf!(
                "Завантаж пакет {file} з {RELEASES} і постав: sudo pacman -U {file}",
                "Download the {file} package from {RELEASES} and install it: sudo pacman -U {file}"
            )
        } else {
            tr!(
                "Онови так само, як встановлював: git pull і ./install.sh у теці з вихідними кодами.",
                "Update the way you installed it: git pull and ./install.sh in the source folder."
            )
            .to_string()
        };
        let mine = &f.self_version;
        out.push(Notice {
            items: vec![("self".into(), latest.to_string())],
            title: trf!("Вийшов OBS-Legacy {}", "OBS-Legacy {} is out", latest.trim_start_matches('v')),
            body: trf!(
                "У тебе {mine}. {how} Твій OBS не зміниться.",
                "You have {mine}. {how} Your OBS stays as it is."
            ),
            urgent: false,
        });
    }
    out
}

/// Рядки проблем з виводу перевірки бібліотек (pacman-хука).
pub fn parse_libcheck(out: &str) -> Vec<String> {
    out.lines()
        .filter(|l| l.starts_with("      "))
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('…'))
        .collect()
}

/// Відбиток списку проблем (FNV-1a): сповіщати знову, лише коли він змінився.
fn fingerprint(problems: &[String]) -> String {
    let mut sorted: Vec<&str> = problems.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in sorted.join("\n").bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Сповіщення про бібліотеки, яких бракує власній збірці.
pub fn libs_notice(problems: &[String]) -> Option<Notice> {
    if problems.is_empty() {
        return None;
    }
    let mut libs: Vec<&str> = problems.iter().map(|p| p.rsplit_once(": ").map_or(p.as_str(), |(_, l)| l)).collect();
    libs.sort_unstable();
    libs.dedup();
    let mut list = libs.iter().take(3).copied().collect::<Vec<_>>().join(", ");
    if libs.len() > 3 {
        let more = libs.len() - 3;
        list.push_str(&trf!(" і ще {more}", " and {more} more"));
    }
    Some(Notice {
        items: vec![("libs".into(), fingerprint(problems))],
        title: tr!("OBS після оновлення системи може не запуститися", "OBS may not start after the system update")
            .into(),
        body: trf!(
            "Власній збірці OBS бракує бібліотек: {list}. Перезбери: {}.",
            "Your OBS build is missing libraries: {list}. Rebuild: {}.",
            how_install()
        ),
        urgent: true,
    })
}

// ------------------------------------------------------------ перевірки

/// Значення налаштування: змінна середовища має пріоритет, як і в скрипті.
fn setting(settings: &Settings, key: &str) -> String {
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
    // ffmpeg-legacy-580xx зібрано зі старішого ffmpeg, ніж тепер у репозиторії.
    // Коли збирання ffmpeg вимкнено, «install» його й не перезбере — тоді мовчимо.
    let ffmpeg_built = active.and_then(|b| b.get("ffmpeg.arch_version")).filter(|v| !v.is_empty()).map(str::to_string);
    let ffmpeg_new = match (&ffmpeg_built, &u.ffmpeg_repo) {
        (Some(built), Some(repo)) if setting(settings, "BUILD_FFMPEG") != "0" && system::pacman_newer(repo, built) => {
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
        self_menu,
        self_package: !self_menu && service::exe_path() == Path::new(PACKAGE_EXE),
        ffmpeg_built,
        ffmpeg_new,
    }
}

/// Перевірити нові версії (мережа: git ls-remote до GitHub).
pub fn gather() -> Facts {
    let (builds, _) = system::read_builds();
    let active = builds.iter().find(|b| b.active);
    let settings = Settings::load();
    let catalog = script::ensure_extracted().ok().and_then(|s| catalog::load(&s).ok()).unwrap_or_default();
    let repos: Vec<(String, String)> =
        tracked_plugins(active, &settings, &catalog).into_iter().map(|(id, _, _, repo)| (id, repo)).collect();
    let u = system::check_updates(&repos, Some(&self_repo()));
    facts_from(active, &u, &settings, &catalog, script::self_source().is_some())
}

/// Чого бракує власній збірці OBS — тією ж перевіркою, що й pacman-хук.
/// None — перевірки немає (хук не встановлено чи вимкнено).
pub fn check_libs() -> Option<Vec<String>> {
    let libcheck = system::libcheck();
    let text = fs::read_to_string(&libcheck).ok()?;
    if !text.contains(system::MARKER) {
        return None;
    }
    let out = Command::new("timeout")
        .args(["300", "bash"])
        .arg(&libcheck)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Some(parse_libcheck(&String::from_utf8_lossy(&out.stdout)))
}

/// Коли pacman востаннє змінював набір пакетів.
fn pacman_mtime() -> Option<u64> {
    fs::metadata(PACMAN_DB).ok().map(|m| m.mtime().max(0) as u64)
}

// -------------------------------------------------------------- журнал

pub fn log_path() -> PathBuf {
    script::state_dir().join("updates.log")
}

/// Дублювати журнал у stdout: у фоновому режимі його збирає служба
/// (dinitctl catlog, journalctl), а в команді для людини він лише заважав би.
static ECHO: AtomicBool = AtomicBool::new(false);

/// Рядок у журнал ~/.local/state/obs-legacy/updates.log.
fn log(msg: &str) {
    let line = format!("{} {msg}\n", local_time(now()));
    if ECHO.load(Ordering::Relaxed) {
        print!("{line}");
        let _ = io::stdout().flush();
    }
    let path = log_path();
    let _ = fs::create_dir_all(script::state_dir());
    // Журнал не росте безкінечно: лишаються останні 400 рядків.
    if fs::metadata(&path).is_ok_and(|m| m.len() > 256 * 1024)
        && let Ok(text) = fs::read_to_string(&path)
    {
        let lines: Vec<&str> = text.lines().collect();
        let _ = fs::write(&path, lines[lines.len().saturating_sub(400)..].join("\n") + "\n");
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

// ---------------------------------------------------------- блокування

fn lock_path() -> PathBuf {
    script::state_dir().join("watch.lock")
}

/// PID фонової перевірки, якщо вона зараз працює.
pub fn running_pid() -> Option<u32> {
    let file = File::open(lock_path()).ok()?;
    // SAFETY: flock на власному дескрипторі; блокування знімається разом із закриттям файлу.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } == 0 {
        return None;
    }
    fs::read_to_string(lock_path()).ok()?.trim().parse().ok()
}

/// Лише одна фонова перевірка: друга (скажімо, і служба, і автозапуск) чекає,
/// поки перша завершиться, і лише тоді береться до роботи.
fn acquire_lock() -> io::Result<File> {
    fs::create_dir_all(script::state_dir())?;
    let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(lock_path())?;
    // SAFETY: flock на власному дескрипторі.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let other = fs::read_to_string(lock_path()).unwrap_or_default();
        log(&trf!(
            "Фонова перевірка вже працює (PID {}) — чекаю, поки вона завершиться.",
            "The background check is already running (PID {}) — waiting for it to end.",
            other.trim()
        ));
        // SAFETY: як вище; цього разу — чекати.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    file.set_len(0)?;
    file.rewind()?;
    writeln!(file, "{}", std::process::id())?;
    Ok(file)
}

// ------------------------------------------------------------- команди

/// Надіслати сповіщення, яких ще не показували; показані — позначити в стані.
/// Повертає ті, що не вдалося показати (немає графічної сесії чи служби сповіщень).
fn deliver(list: Vec<Notice>, st: &mut State, open: Option<&Path>, verbose: bool) -> Vec<Notice> {
    if list.is_empty() {
        return list;
    }
    let Some(session) = notify::find_session() else {
        if verbose {
            cli::warn(tr!(
                "Графічної сесії не знайдено — сповіщення не показано.",
                "No graphical session found — the notification wasn't shown."
            ));
        }
        return list;
    };
    list.into_iter()
        .filter_map(|n| match notify::send(&session, &n.message(open)) {
            Ok(()) => {
                n.mark(st);
                log(&trf!("Сповіщення: «{}» ({})", "Notification: “{}” ({})", n.title, session.describe()));
                if verbose {
                    cli::ok(&trf!("Сповіщення: «{}»", "Notification: “{}”", n.title));
                }
                None
            }
            Err(e) => {
                log(&trf!("Не вдалося показати «{}»: {e}", "Couldn't show “{}”: {e}", n.title));
                if verbose {
                    cli::warn(&trf!(
                        "Не вдалося показати сповіщення «{}»: {e}",
                        "Couldn't show the notification “{}”: {e}",
                        n.title
                    ));
                }
                Some(n)
            }
        })
        .collect()
}

/// Підсумок перевірки рядками [OK]/[УВАГА].
fn report(f: &Facts, libs: Option<&[String]>) {
    match (f.obs_installed.as_deref(), f.obs_latest.as_deref()) {
        (_, None) => cli::warn(tr!(
            "Не вдалося дізнатися найновішу версію OBS: немає мережі чи GitHub недоступний.",
            "Couldn't find out the latest OBS version: no network or GitHub is unreachable."
        )),
        (Some(i), Some(l)) if newer(l, i) => match &f.obs_pinned {
            Some(pin) => cli::ok(&trf!(
                "Вийшов OBS {l}, але версію зафіксовано (OBS_REF={pin}) — про OBS не сповіщаю.",
                "OBS {l} is out, but the version is pinned (OBS_REF={pin}) — no notification about OBS."
            )),
            None => cli::warn(&trf!("Вийшов OBS {l} (у тебе {i}).", "OBS {l} is out (you have {i}).")),
        },
        (Some(i), Some(_)) => cli::ok(&trf!("OBS {i} — найновіший.", "OBS {i} is the latest.")),
        (None, Some(l)) => cli::ok(&trf!(
            "Найновіший OBS — {l}. Власну збірку ще не встановлено.",
            "The latest OBS is {l}. Your own build isn't installed yet."
        )),
    }
    if !f.plugin_updates.is_empty() {
        cli::warn(&trf!("Нові версії плагінів: {}.", "New plugin versions: {}.", plugin_list(&f.plugin_updates)));
    } else if f.plugins_checked > 0 && f.online() {
        cli::ok(&trf!(
            "Плагіни збірки ({}) — найновіші.",
            "The build's plugins ({}) are the latest.",
            f.plugins_checked
        ));
    }
    match f.self_latest.as_deref() {
        Some(l) if newer(l, &f.self_version) => cli::warn(&trf!(
            "Вийшов OBS-Legacy {} (у тебе {}).",
            "OBS-Legacy {} is out (you have {}).",
            l.trim_start_matches('v'),
            f.self_version
        )),
        Some(_) => cli::ok(&trf!("OBS-Legacy {} — найновіший.", "OBS-Legacy {} is the latest.", f.self_version)),
        None => cli::ok(&trf!(
            "OBS-Legacy {}: найновішу версію дізнатися не вдалося (репозиторій закритий чи немає мережі).",
            "OBS-Legacy {}: couldn't find out the latest version (private repository or no network).",
            f.self_version
        )),
    }
    match libs {
        None => {}
        Some([]) => cli::ok(tr!("Бібліотеки власної збірки на місці.", "Your build's libraries are in place.")),
        Some(p) => cli::warn(&trf!(
            "Власній збірці OBS бракує бібліотек: {}.",
            "Your OBS build is missing libraries: {}.",
            p.join("; ")
        )),
    }
}

/// `obs-legacy --check-updates`: перевірити зараз і сповістити про нове.
pub fn check_main() -> ExitCode {
    cli::say(tr!("Перевірка оновлень", "Checking for updates"));
    let facts = gather();
    let libs = check_libs();
    report(&facts, libs.as_deref());
    let mut st = State::load();
    let t = now();
    st.set("checked", t);
    if facts.online() {
        st.set("ok", t);
    }
    st.set("summary", facts.summary());
    if libs.as_ref().is_some_and(Vec::is_empty) {
        st.remove("notified.libs");
    }
    let mut list = notices(&facts);
    list.extend(libs.as_deref().and_then(libs_notice));
    let fresh: Vec<Notice> = list.into_iter().filter(|n| n.is_new(&st)).collect();
    if fresh.is_empty() {
        cli::info(tr!("Нового, про що ще не сповіщав, немає.", "Nothing new that wasn't notified about."));
    }
    deliver(fresh, &mut st, None, true);
    log(&trf!("Перевірка вручну: {}", "Manual check: {}", facts.summary()));
    if let Err(e) = st.save() {
        cli::warn(&trf!("Не вдалося зберегти {}: {e}", "Couldn't save {}: {e}", State::path().display()));
    }
    ExitCode::SUCCESS
}

/// `obs-legacy --watch`: фонова перевірка (її запускає служба користувача).
pub fn watch_main(exe: &Path) -> ExitCode {
    ECHO.store(true, Ordering::Relaxed);
    let _lock = match acquire_lock() {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "obs-legacy: {}",
                trf!("не вдалося створити {}: {e}", "couldn't create {}: {e}", lock_path().display())
            );
            return ExitCode::FAILURE;
        }
    };
    log(&trf!(
        "Фонова перевірка оновлень запущена: OBS-Legacy {}, PID {}.",
        "Background update check started: OBS-Legacy {}, PID {}.",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    ));
    let mut st = State::load();
    let mut next = st.next_check(now());
    let mut pending: Vec<Notice> = Vec::new();
    let add = |pending: &mut Vec<Notice>, st: &State, n: Notice| {
        if n.is_new(st) && !pending.iter().any(|p| p.items == n.items) {
            pending.retain(|p| p.items.first().map(|i| &i.0) != n.items.first().map(|i| &i.0));
            pending.push(n);
        }
    };
    loop {
        // Мову могли змінити в налаштуваннях, поки служба працює.
        crate::i18n::set(Settings::load().ui_lang());
        let mut dirty = false;
        // Після транзакції pacman (і коли вона вже завершилася) — бібліотеки.
        if let Some(m) = pacman_mtime()
            && st.get("pacman") != Some(m.to_string().as_str())
            && !Path::new(PACMAN_LOCK).exists()
        {
            st.set("pacman", m);
            dirty = true;
            match check_libs() {
                Some(p) if p.is_empty() => st.remove("notified.libs"),
                Some(p) => {
                    log(&trf!(
                        "Власній збірці OBS бракує бібліотек: {}",
                        "Your OBS build is missing libraries: {}",
                        p.join("; ")
                    ));
                    if let Some(n) = libs_notice(&p) {
                        add(&mut pending, &st, n);
                    }
                }
                None => {}
            }
        }
        // Нові версії.
        let t = now();
        if t >= next {
            let facts = gather();
            st.set("checked", t);
            if facts.online() {
                st.set("ok", t);
            }
            st.set("summary", facts.summary());
            next = st.next_check(t);
            dirty = true;
            log(&trf!("Перевірка: {}", "Check: {}", facts.summary()));
            for n in notices(&facts) {
                add(&mut pending, &st, n);
            }
        }
        if !pending.is_empty() {
            let before = pending.len();
            pending = deliver(pending, &mut st, Some(exe), false);
            dirty |= pending.len() != before;
        }
        if dirty && let Err(e) = st.save() {
            log(&trf!("Не вдалося зберегти {}: {e}", "Couldn't save {}: {e}", State::path().display()));
        }
        let wait = next.saturating_sub(now()).clamp(1, TICK);
        thread::sleep(Duration::from_secs(wait));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts {
            obs_installed: Some("33.0.0".into()),
            obs_latest: Some("33.0.0".into()),
            plugins_checked: 2,
            self_version: "0.0.1".into(),
            self_latest: Some("v0.0.1".into()),
            self_menu: true,
            ..Default::default()
        }
    }

    fn blur(latest: &str) -> PluginUpdate {
        PluginUpdate {
            id: "composite-blur".into(),
            name: "Composite Blur".into(),
            installed: "v1.6.1".into(),
            latest: latest.into(),
        }
    }

    #[test]
    fn nothing_new() {
        let f = facts();
        assert!(notices(&f).is_empty());
        assert_eq!(f.summary(), "нового немає");
        let offline = Facts { obs_latest: None, ..facts() };
        assert!(!offline.online());
        assert!(offline.summary().starts_with("не вдалося перевірити"));
    }

    #[test]
    fn obs_update_carries_plugins() {
        let f = Facts { obs_latest: Some("33.0.1".into()), plugin_updates: vec![blur("v1.6.2")], ..facts() };
        let n = notices(&f);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].title, "Вийшов OBS 33.0.1");
        assert!(n[0].body.contains("У тебе OBS 33.0.0"));
        assert!(n[0].body.contains("Composite Blur v1.6.2"));
        assert_eq!(n[0].items, [("obs".into(), "33.0.1".into()), ("plugin.composite-blur".into(), "v1.6.2".into())]);
        assert_eq!(f.summary(), "вийшов OBS 33.0.1; нові версії плагінів: Composite Blur v1.6.2");
    }

    #[test]
    fn pinned_or_older_obs_is_quiet() {
        let pinned = Facts { obs_latest: Some("33.0.1".into()), obs_pinned: Some("32.2.2".into()), ..facts() };
        assert!(notices(&pinned).is_empty());
        let older = Facts { obs_latest: Some("32.2.2".into()), ..facts() };
        assert!(notices(&older).is_empty());
        let no_build = Facts { obs_installed: None, obs_latest: Some("33.0.1".into()), ..facts() };
        assert!(notices(&no_build).is_empty());
    }

    #[test]
    fn plugin_only_updates() {
        let one = notices(&Facts { plugin_updates: vec![blur("v1.6.2")], ..facts() });
        assert_eq!(one[0].title, "Нова версія плагіна Composite Blur: v1.6.2");
        assert!(one[0].body.starts_with("У тебе v1.6.1."));
        let wave = PluginUpdate {
            id: "waveform".into(),
            name: "Waveform".into(),
            installed: "v1.9.1".into(),
            latest: "v1.10.0".into(),
        };
        let two = notices(&Facts { plugin_updates: vec![blur("v1.6.2"), wave], ..facts() });
        assert_eq!(two[0].title, "Нові версії плагінів OBS: 2");
        assert!(two[0].body.starts_with("Composite Blur v1.6.2, Waveform v1.10.0."));
    }

    #[test]
    fn self_update() {
        let n = notices(&Facts { self_latest: Some("v0.0.2".into()), ..facts() });
        assert_eq!(n[0].title, "Вийшов OBS-Legacy 0.0.2");
        assert!(n[0].body.contains("«Оновити OBS-Legacy»"));
        let n = notices(&Facts { self_latest: Some("v0.0.2".into()), self_menu: false, ..facts() });
        assert!(n[0].body.contains("./install.sh"));
        let n = notices(&Facts { self_latest: Some("v0.0.3".into()), self_menu: false, self_package: true, ..facts() });
        assert!(n[0].body.contains("sudo pacman -U obs-legacy-0.0.3-1-x86_64.pkg.tar.zst"));
    }

    #[test]
    fn ffmpeg_update() {
        let f = Facts { ffmpeg_built: Some("2:9.0.2-2".into()), ffmpeg_new: Some("2:9.0.3-1".into()), ..facts() };
        let n = notices(&f);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].items, [("ffmpeg".to_string(), "2:9.0.3-1".to_string())]);
        assert!(n[0].title.contains("2:9.0.3-1") && n[0].body.contains("2:9.0.2-2"));
        assert!(f.summary().contains("новий ffmpeg 2:9.0.3-1"));
        // Зібрано з поточної версії — новин немає.
        assert!(notices(&Facts { ffmpeg_built: Some("2:9.0.2-2".into()), ..facts() }).is_empty());
    }

    #[test]
    fn notified_once() {
        let f = Facts { obs_latest: Some("33.0.1".into()), plugin_updates: vec![blur("v1.6.2")], ..facts() };
        let mut st = State::default();
        let n = &notices(&f)[0];
        assert!(n.is_new(&st));
        n.mark(&mut st);
        assert!(!n.is_new(&st));
        // Новий реліз плагіна — нове сповіщення.
        let f2 = Facts { plugin_updates: vec![blur("v1.6.3")], ..f };
        assert!(notices(&f2)[0].is_new(&st));
        // Стан переживає збереження й читання.
        let text: String = st.map.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
        assert!(!n.is_new(&State::parse(&format!("# коментар\n{text}"))));
    }

    #[test]
    fn libcheck_output() {
        let out = "\n==> OBS-Legacy: власній збірці OBS після оновлення бракує бібліотек:\n      \
                   bin/obs: libavcodec.so.61\n      lib/libobs.so.30: libavcodec.so.61\n      \
                   lib/obs-plugins/x.so: libQt6Core.so.6 (Qt_6.9)\n      … і ще 2\n    Перезбери OBS: x\n";
        let p = parse_libcheck(out);
        assert_eq!(p.len(), 3);
        let n = libs_notice(&p).unwrap();
        assert!(n.urgent);
        assert!(n.body.contains("libQt6Core.so.6 (Qt_6.9), libavcodec.so.61."));
        // Відбиток не залежить від порядку.
        let mut rev = p.clone();
        rev.reverse();
        assert_eq!(fingerprint(&p), fingerprint(&rev));
        assert!(libs_notice(&[]).is_none());
        assert!(parse_libcheck("").is_empty());
    }

    #[test]
    fn schedule() {
        let mut st = State::default();
        assert_eq!(st.next_check(1000), 1000 + FIRST_DELAY);
        st.set("checked", 5000);
        st.set("ok", 5000);
        assert_eq!(st.next_check(6000), 5000 + PERIOD);
        st.set("checked", 9000);
        assert_eq!(st.next_check(9000), 9000 + RETRY);
        // Давно не перевіряли — за 2 хв після запуску.
        assert_eq!(st.next_check(9000 + PERIOD * 3), 9000 + PERIOD * 3 + FIRST_DELAY);
    }

    #[test]
    fn time_format() {
        let s = local_time(1_761_740_000);
        assert_eq!(s.len(), 16);
        assert!(s.starts_with("2025-"));
    }
}
