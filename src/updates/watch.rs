//! `--check-updates` і фонова `--watch`: перевірки за розкладом, журнал, блокування (лише одна фонова
//! перевірка) і доставка сповіщень у графічну сесію.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use super::auto;
use super::notices::{
    Notice, fingerprint, kept_notice, libs_notice, notices, prepare_failed, prepare_ready, prepare_ready_ffmpeg,
    prepare_started,
};
use super::prepare::{self, Reasons};
use super::state::State;
use super::{Facts, check_libs, facts_offline, ffmpeg_new_local, gather, gather_with_updates, kept_libs, plugin_list};
use crate::cli;
use crate::clock;
use crate::notify;
use crate::paths;
use crate::reports;
use crate::script;
use crate::settings::Settings;
use crate::versions::{Updates, newer};

const PACMAN_DB: &str = "/var/lib/pacman/local";

const PACMAN_LOCK: &str = "/var/lib/pacman/db.lck";

/// Як часто прокидатися (бібліотеки після pacman, сповіщення, що чекають сесії). Хвилина — щоб
/// перезбирання після `pacman -Syu` починалося майже одразу; поза транзакціями пробудження дешеве
/// (кілька stat).
const TICK: u64 = 60;

/// Журнал не росте безкінечно: більший за LOG_MAX_BYTES обрізається до останніх LOG_KEEP_LINES рядків.
const LOG_MAX_BYTES: u64 = 256 * 1024;
const LOG_KEEP_LINES: usize = 400;

/// Коли pacman востаннє змінював набір пакетів.
fn pacman_mtime() -> Option<u64> {
    fs::metadata(PACMAN_DB).ok().map(|m| m.mtime().max(0) as u64)
}

pub fn log_path() -> PathBuf {
    paths::state_dir().join("updates.log")
}

/// Дублювати журнал у stdout: у фоновому режимі його збирає служба
/// (dinitctl catlog, journalctl), а в команді для людини він лише заважав би.
static ECHO: AtomicBool = AtomicBool::new(false);

/// Рядок у журнал ~/.local/state/obs-legacy/updates.log.
fn log(msg: &str) {
    let line = format!("{} {msg}\n", clock::local_time(clock::now()));
    if ECHO.load(Ordering::Relaxed) {
        print!("{line}");
        let _ = io::stdout().flush();
    }
    let path = log_path();
    let _ = fs::create_dir_all(paths::state_dir());
    if fs::metadata(&path).is_ok_and(|m| m.len() > LOG_MAX_BYTES)
        && let Ok(text) = fs::read_to_string(&path)
    {
        let lines: Vec<&str> = text.lines().collect();
        let _ = fs::write(&path, lines[lines.len().saturating_sub(LOG_KEEP_LINES)..].join("\n") + "\n");
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn lock_path() -> PathBuf {
    paths::state_dir().join("watch.lock")
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
    fs::create_dir_all(paths::state_dir())?;
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
    st.record_check(clock::now(), &facts);
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
    let st = State::load();
    let next = st.next_check(clock::now());
    let libs = check_libs().filter(|p| !p.is_empty());
    let mut watch = Watch {
        exe,
        st,
        next,
        pending: Vec::new(),
        ffmpeg_new: ffmpeg_new_local(),
        libs,
        kept: kept_libs(),
        facts: None,
        updates: None,
        upgraded: false,
        prep: None,
        prep_key: String::new(),
    };
    loop {
        let wait = watch.tick();
        thread::sleep(wait);
    }
}

/// Додати сповіщення в чергу, якщо про це ще не сповіщали. Новіше про те саме (той самий перший ключ:
/// obs, ffmpeg, libs…) замінює старіше, що ще чекає на графічну сесію.
fn queue(pending: &mut Vec<Notice>, st: &State, n: Notice) {
    if n.is_new(st) && !pending.iter().any(|p| p.items == n.items) {
        pending.retain(|p| p.items.first().map(|i| &i.0) != n.items.first().map(|i| &i.0));
        pending.push(n);
    }
}

/// Стан фонової перевірки між пробудженнями.
struct Watch<'a> {
    exe: &'a Path,
    st: State,
    /// Коли перевіряти нові версії наступного разу (секунди від епохи).
    next: u64,
    /// Сповіщення, які ще не вдалося показати (немає графічної сесії).
    pending: Vec<Notice>,
    /// Новий ffmpeg у репозиторії, з якого ffmpeg-legacy-580xx ще не зібрано.
    ffmpeg_new: Option<String>,
    /// Чого бракує пакетам OBS-Legacy після останньої транзакції pacman.
    libs: Option<Vec<String>>,
    /// Бібліотеки, на збережених копіях яких працюють пакети OBS-Legacy (хук keep-libs).
    kept: Vec<String>,
    /// Останні відомі нові версії: факти й відповідь GitHub (щоб перераховувати без мережі).
    facts: Option<Facts>,
    updates: Option<Updates>,
    /// Після останньої підготовки була транзакція pacman (оновлення системи): нові OBS, плагіни
    /// й ffmpeg збираються саме тоді — разом з оновленням.
    upgraded: bool,
    /// Фонова підготовка пакетів (`prepare`), що зараз іде, і її причина.
    prep: Option<Child>,
    prep_key: String,
}

impl Watch<'_> {
    /// Одне пробудження: бібліотеки, нові версії, сповіщення. Повертає, скільки спати до наступного.
    fn tick(&mut self) -> Duration {
        // Мову могли змінити в налаштуваннях, поки служба працює.
        crate::i18n::set(Settings::load().ui_lang());
        let libs = self.check_libs_after_pacman();
        let versions = self.check_versions();
        let prepared = self.run_prepare();
        let delivered = self.deliver_pending();
        if (libs || versions || prepared || delivered)
            && let Err(e) = self.st.save()
        {
            log(&trf!("Не вдалося зберегти {}: {e}", "Couldn't save {}: {e}", State::path().display()));
        }
        Duration::from_secs(self.next.saturating_sub(clock::now()).clamp(1, TICK))
    }

    /// Після транзакції pacman (і коли вона вже завершилася) — чи вистачає бібліотек. true — стан змінився.
    fn check_libs_after_pacman(&mut self) -> bool {
        let Some(m) = pacman_mtime() else { return false };
        if self.st.get("pacman") == Some(m.to_string().as_str()) || Path::new(PACMAN_LOCK).exists() {
            return false;
        }
        self.st.set("pacman", m);
        self.upgraded = true;
        // pacman -Sy користувача міг принести новий ffmpeg, а pacman -U — новий набір: це видно й без мережі.
        match &self.updates {
            Some(u) => {
                let f = facts_offline(u);
                self.ffmpeg_new = f.ffmpeg_new.clone();
                self.facts = Some(f);
            }
            None => self.ffmpeg_new = ffmpeg_new_local(),
        }
        self.kept = kept_libs();
        if self.kept.is_empty() {
            self.st.remove("notified.kept");
        } else {
            log(&trf!(
                "Пакети OBS-Legacy працюють на збережених копіях бібліотек: {}",
                "The OBS-Legacy packages run on kept library copies: {}",
                self.kept.join("; ")
            ));
            // Добудовувач з пакета obs-legacy сам перезбирає й ставить їх і сам про це сповіщає.
            if let Some(n) = kept_notice(&self.kept).filter(|_| !auto::installed()) {
                queue(&mut self.pending, &self.st, n);
            }
        }
        let libs = check_libs();
        self.libs = libs.clone().filter(|p| !p.is_empty());
        match libs {
            Some(p) if p.is_empty() => self.st.remove("notified.libs"),
            Some(p) => {
                log(&trf!(
                    "Власній збірці OBS бракує бібліотек: {}",
                    "Your OBS build is missing libraries: {}",
                    p.join("; ")
                ));
                if let Some(n) = libs_notice(&p).filter(|_| !auto::installed()) {
                    queue(&mut self.pending, &self.st, n);
                }
            }
            None => {}
        }
        true
    }

    /// Нові версії, коли настав час. true — стан змінився.
    fn check_versions(&mut self) -> bool {
        let t = clock::now();
        if t < self.next {
            return false;
        }
        sync_repo_copy();
        let (facts, u) = gather_with_updates();
        self.ffmpeg_new = facts.ffmpeg_new.clone();
        self.st.record_check(t, &facts);
        self.next = self.st.next_check(t);
        log(&trf!("Перевірка: {}", "Check: {}", facts.summary()));
        for n in notices(&facts) {
            queue(&mut self.pending, &self.st, n);
        }
        self.facts = Some(facts);
        self.updates = Some(u);
        true
    }

    /// Чому зараз готувати пакети. Збережені копії й бібліотеки, яких бракує, — одразу; нові ffmpeg,
    /// OBS і плагіни — лише після транзакції pacman, тобто разом з оновленням системи. Коли стоїть
    /// добудовувач (`auto::installed`), бібліотеки — його справа: він лагодить і ставить у тій самій
    /// транзакції, тож тут їх не збирати вдруге.
    fn reasons(&self) -> Reasons {
        let with_upgrade = |v: Option<String>| v.filter(|_| self.upgraded);
        let facts = self.facts.as_ref();
        let auto = auto::installed();
        Reasons {
            ffmpeg: with_upgrade(self.ffmpeg_new.clone()),
            libs: self.libs.as_deref().filter(|_| !auto).map(fingerprint),
            kept: (!self.kept.is_empty() && !auto).then(|| fingerprint(&self.kept)),
            obs: with_upgrade(facts.and_then(Facts::obs_update).map(str::to_string)),
            plugins: with_upgrade(
                facts.filter(|f| !f.plugin_updates.is_empty()).map(|f| plugin_list(&f.plugin_updates)),
            ),
        }
    }

    /// Фонова підготовка пакетів: дочекатися тієї, що йде, або почати нову, коли є причина
    /// (новий ffmpeg, бракує бібліотек), її ще не пробували, а скрипт і pacman вільні. true — стан змінився.
    fn run_prepare(&mut self) -> bool {
        if let Some(child) = &mut self.prep {
            return match child.try_wait() {
                Ok(None) => false,
                Ok(Some(status)) => {
                    self.prep = None;
                    self.finish_prepare(status.success());
                    true
                }
                Err(e) => {
                    log(&trf!("Фонова підготовка пакетів: {e}", "Background package preparation: {e}"));
                    self.prep = None;
                    false
                }
            };
        }
        let reasons = self.reasons();
        let Some(key) = reasons.key() else { return false };
        if self.st.get("prepared") == Some(key.as_str())
            || !prepare::enabled(&Settings::load())
            || Path::new(PACMAN_LOCK).exists()
            || prepare::script_busy()
        {
            return false;
        }
        let child = match script::ensure_extracted().and_then(|s| Ok(prepare::spawn(&s, "prepare")?)) {
            Ok(c) => c,
            Err(e) => {
                log(&trf!("Не вдалося почати підготовку пакетів: {e:#}", "Couldn't start preparing packages: {e:#}"));
                return false;
            }
        };
        log(&trf!(
            "Готую пакети у фоні (PID {}): {key}",
            "Preparing packages in the background (PID {}): {key}",
            child.id()
        ));
        // Одна спроба на причину: невдале збирання не повторюється по колу.
        self.st.set("prepared", &key);
        self.prep = Some(child);
        self.prep_key = key.clone();
        self.upgraded = false;
        // Про те, що збирає ця підготовка, окремо не сповіщати: про все скаже вона сама.
        if let Some(v) = &reasons.ffmpeg {
            self.st.set("notified.ffmpeg", v);
        }
        if let Some(n) = self.libs.as_deref().and_then(libs_notice) {
            n.mark(&mut self.st);
        }
        if let Some(n) = kept_notice(&self.kept) {
            n.mark(&mut self.st);
        }
        if (reasons.obs.is_some() || reasons.plugins.is_some())
            && let Some(f) = &self.facts
        {
            for (k, v) in notices(f).iter().flat_map(|n| n.items.iter()) {
                if k == "obs" || k.starts_with("plugin.") {
                    self.st.set(&format!("notified.{k}"), v);
                }
            }
        }
        let st = &self.st;
        self.pending.retain(|n| n.is_new(st));
        let started = prepare_started(&key, &reasons);
        queue(&mut self.pending, &self.st, started);
        true
    }

    /// Підготовка завершилася: сповістити про готовий набір чи про помилку (з порадою зі звіту).
    fn finish_prepare(&mut self, success: bool) {
        let key = std::mem::take(&mut self.prep_key);
        if success {
            match prepare::ready_label() {
                Some(label) => {
                    log(&trf!("Пакети готові: {label}", "Packages are ready: {label}"));
                    queue(&mut self.pending, &self.st, prepare_ready(&label));
                }
                None => match prepare::ready_ffmpeg() {
                    Some(version) => {
                        log(&trf!("Готовий лише ffmpeg {version}", "Only ffmpeg {version} is ready"));
                        queue(&mut self.pending, &self.st, prepare_ready_ffmpeg(&version));
                    }
                    None => log(tr!("Підготовка: готувати нічого.", "Preparation: nothing to prepare.")),
                },
            }
            return;
        }
        let report = reports::list().into_iter().find(|r| r.command == "prepare");
        let problem = report.as_ref().and_then(|r| r.problem.as_ref());
        log(&trf!(
            "Підготовка пакетів не вдалася: {}",
            "Preparing packages failed: {}",
            problem.map_or("?", |p| p.text.as_str())
        ));
        let n = prepare_failed(
            &key,
            problem.map(|p| p.text.trim_end_matches('.')),
            problem.and_then(|p| p.hints.first()).map(String::as_str),
        );
        queue(&mut self.pending, &self.st, n);
    }

    /// Показати сповіщення, що чекають. true — щось показано.
    fn deliver_pending(&mut self) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        let before = self.pending.len();
        self.pending = deliver(std::mem::take(&mut self.pending), &mut self.st, Some(self.exe), false);
        self.pending.len() != before
    }
}

/// Оновити власну копію баз pacman (команда скрипта `repo-sync`, без root): про новий ffmpeg видно
/// ще до того, як людина запустить `pacman -Syu`. Не вдалося (мережа) — лишається системна база.
fn sync_repo_copy() {
    let Ok(script) = script::ensure_extracted() else { return };
    let ok = Command::new("bash")
        .arg(&script)
        .arg("repo-sync")
        .env("OBS_LEGACY_LANG", crate::i18n::lang().code())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        log(tr!(
            "Не вдалося оновити копію баз pacman (repo-sync) — беру системну.",
            "Couldn't refresh the pacman database copy (repo-sync) — using the system one."
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(key: &str, value: &str) -> Notice {
        Notice { items: vec![(key.into(), value.into())], title: value.into(), body: String::new(), urgent: false }
    }

    #[test]
    fn newer_news_replaces_the_waiting_one() {
        let mut st = State::default();
        let mut pending = Vec::new();
        queue(&mut pending, &st, notice("obs", "33.0.1"));
        queue(&mut pending, &st, notice("obs", "33.0.1"));
        queue(&mut pending, &st, notice("ffmpeg", "2:9.0.3-1"));
        assert_eq!(pending.len(), 2);
        // Поки сповіщення чекало на сесію, вийшла ще новіша версія — показати лише її.
        queue(&mut pending, &st, notice("obs", "33.0.2"));
        assert_eq!(pending.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(), ["2:9.0.3-1", "33.0.2"]);
        // Про показане вже не сповіщаємо.
        notice("plugin.waveform", "v1.10.0").mark(&mut st);
        queue(&mut pending, &st, notice("plugin.waveform", "v1.10.0"));
        assert_eq!(pending.len(), 2);
    }
}
