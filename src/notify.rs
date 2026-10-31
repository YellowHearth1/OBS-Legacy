//! Сповіщення на робочому столі.
//!
//! Фонова перевірка оновлень працює як служба користувача (dinit, systemd,
//! OpenRC, runit, s6) — поза графічною сесією: у її середовищі немає ні
//! WAYLAND_DISPLAY/DISPLAY, ні адреси шини D-Bus, на якій живе служба
//! сповіщень. Шина служб і шина графічної сесії до того ж бувають різними
//! (так у userspawn і turnstile на Artix). Тож середовище сесії береться з
//! процесів самого користувача (/proc/<pid>/environ): перемагає комбінація
//! шини й дисплея, яка трапляється найчастіше.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::system;

/// Змінні, які переносяться з графічної сесії в notify-send і в OBS-Legacy,
/// що відкривається клацанням по сповіщенню.
const SESSION_VARS: &[&str] = &[
    "DBUS_SESSION_BUS_ADDRESS",
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    "XDG_SESSION_DESKTOP",
    "DESKTOP_SESSION",
    "XDG_DATA_DIRS",
    "PATH",
];

/// Підпис кнопки на сповіщенні.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Session {
    pub vars: Vec<(String, String)>,
}

impl Session {
    fn from_map(map: &HashMap<String, String>) -> Self {
        let vars = SESSION_VARS
            .iter()
            .filter_map(|k| map.get(*k).filter(|v| !v.is_empty()).map(|v| (k.to_string(), v.clone())))
            .collect();
        Self { vars }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// Запускати програму в цій сесії: її змінні замість власних.
    fn apply(&self, cmd: &mut Command) {
        for key in SESSION_VARS {
            cmd.env_remove(key);
        }
        for (k, v) in &self.vars {
            cmd.env(k, v);
        }
    }

    /// Коротко для журналу: «Wayland wayland-1, шина unix:path=/tmp/dbus-…».
    pub fn describe(&self) -> String {
        let display = match (self.get("WAYLAND_DISPLAY"), self.get("DISPLAY")) {
            (Some(w), _) => format!("Wayland {w}"),
            (None, Some(d)) => format!("X11 {d}"),
            _ => tr!("без дисплея", "no display").into(),
        };
        let bus = self
            .get("DBUS_SESSION_BUS_ADDRESS")
            .map(|b| b.split(',').next().unwrap_or(b))
            .unwrap_or(tr!("типова", "default"));
        trf!("{display}, шина {bus}", "{display}, bus {bus}")
    }
}

fn graphical(vars: &HashMap<String, String>) -> bool {
    ["WAYLAND_DISPLAY", "DISPLAY"].iter().any(|k| vars.get(*k).is_some_and(|v| !v.is_empty()))
}

/// Чи існує сокет шини (для `unix:path=`); інші види адрес перевірити не можна.
fn bus_alive(addr: Option<&str>) -> bool {
    let Some(addr) = addr else { return true };
    let Some(rest) = addr.split(';').next().and_then(|a| a.strip_prefix("unix:")) else { return true };
    match rest.split(',').find_map(|kv| kv.strip_prefix("path=")) {
        Some(path) => Path::new(path).exists(),
        None => true,
    }
}

fn parse_environ(raw: &[u8]) -> HashMap<String, String> {
    raw.split(|b| *b == 0)
        .filter_map(|kv| {
            let (k, v) = std::str::from_utf8(kv).ok()?.split_once('=')?;
            SESSION_VARS.contains(&k).then(|| (k.to_string(), v.to_string()))
        })
        .collect()
}

/// Середовища графічних процесів користувача (від найстаріших до найновіших).
fn scan_proc() -> Vec<HashMap<String, String>> {
    // SAFETY: getuid не має передумов.
    let uid = unsafe { libc::getuid() };
    let me = std::process::id();
    let mut pids: Vec<u32> = fs::read_dir("/proc")
        .map(|rd| rd.filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok()).collect())
        .unwrap_or_default();
    pids.sort_unstable();
    pids.into_iter()
        .filter(|&pid| pid != me)
        .filter_map(|pid| {
            let dir = PathBuf::from(format!("/proc/{pid}"));
            if fs::metadata(&dir).ok()?.uid() != uid {
                return None;
            }
            let vars = parse_environ(&fs::read(dir.join("environ")).ok()?);
            graphical(&vars).then_some(vars)
        })
        .collect()
}

/// Найчастіша комбінація шини й дисплея; за рівності — новіша (більший PID).
fn pick(envs: &[HashMap<String, String>]) -> Option<Session> {
    let key = |m: &HashMap<String, String>| {
        ["DBUS_SESSION_BUS_ADDRESS", "WAYLAND_DISPLAY", "DISPLAY"].map(|k| m.get(k).cloned().unwrap_or_default())
    };
    let mut counts: HashMap<[String; 3], (usize, usize)> = HashMap::new();
    for (i, m) in envs.iter().enumerate() {
        if !bus_alive(m.get("DBUS_SESSION_BUS_ADDRESS").map(String::as_str)) {
            continue;
        }
        let e = counts.entry(key(m)).or_insert((0, i));
        e.0 += 1;
        e.1 = i;
    }
    let (_, (_, last)) = counts.into_iter().max_by_key(|(_, (n, last))| (*n, *last))?;
    Some(Session::from_map(&envs[last]))
}

/// Графічна сесія користувача: власне середовище (запуск із сесії чи
/// терміналу) або середовище графічних процесів (запуск службою).
pub fn find_session() -> Option<Session> {
    let own: HashMap<String, String> = env::vars().collect();
    if graphical(&own) && bus_alive(own.get("DBUS_SESSION_BUS_ADDRESS").map(String::as_str)) {
        return Some(Session::from_map(&own));
    }
    pick(&scan_proc())
}

pub struct Message<'a> {
    pub title: &'a str,
    pub body: &'a str,
    /// Важливе (критичне) сповіщення: зазвичай не зникає саме.
    pub urgent: bool,
    /// Кнопка «Відкрити OBS-Legacy»: клацання запускає цю програму.
    pub open: Option<&'a Path>,
}

/// Іконка: файл, який ставить install.sh, або назва з теми значків.
fn icon() -> String {
    let data = env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map_or_else(|| crate::script::home().join(".local/share"), PathBuf::from);
    let file = data.join("icons/hicolor/scalable/apps/obs-legacy.svg");
    if file.is_file() { file.display().to_string() } else { "obs-legacy".into() }
}

fn read_all(r: Option<impl Read>) -> String {
    let mut s = String::new();
    if let Some(mut r) = r {
        let _ = r.read_to_string(&mut s);
    }
    s
}

/// Показати сповіщення в сесії. Ok — служба сповіщень його прийняла.
pub fn send(session: &Session, msg: &Message) -> Result<(), String> {
    let mut errors = Vec::new();
    if system::find_in_path("notify-send").is_some() {
        match notify_send(session, msg, msg.open.is_some()) {
            Ok(()) => return Ok(()),
            Err(e) => errors.push(e),
        }
        // Старий libnotify без --action чи служба без кнопок — без кнопки.
        if msg.open.is_some() {
            match notify_send(session, msg, false) {
                Ok(()) => return Ok(()),
                Err(e) => errors.push(e),
            }
        }
    }
    match gdbus(session, msg) {
        Ok(()) => Ok(()),
        Err(e) => {
            errors.push(e);
            Err(errors.join("; "))
        }
    }
}

fn notify_send(session: &Session, msg: &Message, with_action: bool) -> Result<(), String> {
    let mut cmd = Command::new("notify-send");
    session.apply(&mut cmd);
    cmd.arg("--app-name=OBS-Legacy")
        .arg(format!("--icon={}", icon()))
        .arg(format!("--urgency={}", if msg.urgent { "critical" } else { "normal" }))
        .arg("--hint=string:desktop-entry:obs-legacy");
    if with_action {
        cmd.arg(format!("--action=open={}", tr!("Відкрити OBS-Legacy", "Open OBS-Legacy")));
    }
    cmd.arg("--").arg(msg.title).arg(msg.body).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("notify-send: {e}"))?;
    // Без кнопки notify-send завершується одразу. З кнопкою — чекає, поки
    // сповіщення закриють, тож «показано» означає: за 3 с не завершився з помилкою.
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = read_all(child.stdout.take());
                let err = read_all(child.stderr.take());
                if !status.success() {
                    let err = err.trim();
                    return Err(if err.is_empty() {
                        trf!("notify-send: код {status}", "notify-send: code {status}")
                    } else {
                        err.to_string()
                    });
                }
                if with_action && out.trim() == "open" {
                    open_app(msg.open, session);
                }
                return Ok(());
            }
            Ok(None) if start.elapsed() < Duration::from_secs(3) => thread::sleep(Duration::from_millis(100)),
            Ok(None) => {
                if with_action {
                    let open = msg.open.map(Path::to_path_buf);
                    let session = session.clone();
                    thread::spawn(move || wait_click(child, open, session));
                }
                return Ok(());
            }
            Err(e) => return Err(format!("notify-send: {e}")),
        }
    }
}

/// Дочекатися, чим закінчиться сповіщення з кнопкою.
fn wait_click(child: Child, open: Option<PathBuf>, session: Session) {
    if let Ok(out) = child.wait_with_output()
        && String::from_utf8_lossy(&out.stdout).trim() == "open"
    {
        open_app(open.as_deref(), &session);
    }
}

/// Відкрити OBS-Legacy в сесії (без термінала він сам відкриє вікно термінала).
fn open_app(exe: Option<&Path>, session: &Session) {
    let Some(exe) = exe else { return };
    let mut cmd = Command::new(exe);
    session.apply(&mut cmd);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // SAFETY: setsid безпечний між fork і exec; так вікно не залежить від служби.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    if let Ok(mut child) = cmd.spawn() {
        thread::spawn(move || child.wait());
    }
}

/// Рядок у текстовому форматі GVariant.
fn gv_str(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Запасний шлях без libnotify: gdbus з glib2 (кнопки не буде).
fn gdbus(session: &Session, msg: &Message) -> Result<(), String> {
    if system::find_in_path("gdbus").is_none() {
        return Err(tr!(
            "немає ні notify-send (пакет libnotify), ні gdbus (glib2)",
            "neither notify-send (package libnotify) nor gdbus (glib2) is available"
        )
        .into());
    }
    let urgency = if msg.urgent { 2 } else { 1 };
    let mut cmd = Command::new("gdbus");
    session.apply(&mut cmd);
    let out = cmd
        .args([
            "call",
            "--session",
            "--dest=org.freedesktop.Notifications",
            "--object-path=/org/freedesktop/Notifications",
            "--method=org.freedesktop.Notifications.Notify",
            "OBS-Legacy",
            "0",
        ])
        .arg(gv_str(&icon()))
        .arg(gv_str(msg.title))
        .arg(gv_str(msg.body))
        .arg("[]")
        .arg(format!("{{'urgency': <byte {urgency}>, 'desktop-entry': <'obs-legacy'>}}"))
        .arg("-1")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("gdbus: {e}"))?;
    if out.status.success() { Ok(()) } else { Err(format!("gdbus: {}", String::from_utf8_lossy(&out.stderr).trim())) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn environ_keeps_session_vars_only() {
        let m =
            parse_environ(b"HOME=/home/u\0WAYLAND_DISPLAY=wayland-1\0DBUS_SESSION_BUS_ADDRESS=unix:path=/x,guid=1\0");
        assert_eq!(m.len(), 2);
        assert_eq!(m["WAYLAND_DISPLAY"], "wayland-1");
        assert!(graphical(&m));
        assert!(!graphical(&env(&[("DISPLAY", "")])));
    }

    #[test]
    fn bus_address_check() {
        assert!(bus_alive(None));
        assert!(bus_alive(Some("unix:abstract=/tmp/dbus-x,guid=1")));
        assert!(bus_alive(Some("unix:path=/proc/self/environ")));
        assert!(!bus_alive(Some("unix:path=/nonexistent/obs-legacy-bus,guid=1")));
    }

    #[test]
    fn majority_session_wins() {
        let a = env(&[("WAYLAND_DISPLAY", "wayland-1"), ("DBUS_SESSION_BUS_ADDRESS", "unix:abstract=a")]);
        let b = env(&[("WAYLAND_DISPLAY", "wayland-1"), ("DBUS_SESSION_BUS_ADDRESS", "unix:abstract=b")]);
        let dead = env(&[("DISPLAY", ":0"), ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus")]);
        let s = pick(&[a.clone(), b.clone(), b.clone(), dead.clone(), dead.clone(), dead]).unwrap();
        assert_eq!(s.get("DBUS_SESSION_BUS_ADDRESS"), Some("unix:abstract=b"));
        // За рівності — новіший процес.
        let s = pick(&[a.clone(), b]).unwrap();
        assert_eq!(s.get("DBUS_SESSION_BUS_ADDRESS"), Some("unix:abstract=b"));
        assert_eq!(pick(&[]), None);
        assert_eq!(s.describe(), "Wayland wayland-1, шина unix:abstract=b");
    }

    #[test]
    fn gvariant_strings_are_escaped() {
        assert_eq!(gv_str("OBS 33.0.1"), "'OBS 33.0.1'");
        assert_eq!(gv_str(r"it's \ ok"), r"'it\'s \\ ok'");
    }
}
