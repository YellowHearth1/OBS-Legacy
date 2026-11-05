//! `--check-updates` і фонова `--watch`: перевірки за розкладом, журнал, блокування (лише одна фонова
//! перевірка) і доставка сповіщень у графічну сесію.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use super::notices::{Notice, libs_notice, notices};
use super::state::State;
use super::{Facts, check_libs, gather, plugin_list};
use crate::cli;
use crate::clock;
use crate::notify;
use crate::paths;
use crate::settings::Settings;
use crate::versions::newer;

const PACMAN_DB: &str = "/var/lib/pacman/local";

const PACMAN_LOCK: &str = "/var/lib/pacman/db.lck";

/// Як часто прокидатися (бібліотеки після pacman, сповіщення, що чекають сесії).
const TICK: u64 = 5 * 60;

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
    let t = clock::now();
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
    let mut next = st.next_check(clock::now());
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
        let t = clock::now();
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
        let wait = next.saturating_sub(clock::now()).clamp(1, TICK);
        thread::sleep(Duration::from_secs(wait));
    }
}
