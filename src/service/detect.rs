//! Які служби користувача тут працюють (systemd, dinit, OpenRC, runit, s6, автозапуск) і де вже
//! стоїть наша.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::{Backend, NAME, Place, quiet_ok};
use crate::paths::{self, find_in_path};

/// Тека служб з командного рядка runsvdir (`runsvdir [-P] тека [log]`) чи
/// s6-svscan (`s6-svscan [-c n] [-t n] … [тека]`).
fn scan_dir_arg(args: &[String], s6: bool) -> Option<PathBuf> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a.len() > 1 && a.starts_with('-') {
            if s6 && a.len() == 2 && "cCLtdX".contains(&a[1..]) {
                it.next();
            }
            continue;
        }
        return Some(PathBuf::from(a));
    }
    None
}

/// Теки служб, які обслуговують runsvdir чи s6-svscan цього користувача.
fn supervisor_dirs(program: &str) -> Vec<PathBuf> {
    // SAFETY: getuid не має передумов.
    let uid = unsafe { libc::getuid() };
    let mut dirs: Vec<PathBuf> = Vec::new();
    let Ok(rd) = fs::read_dir("/proc") else { return dirs };
    for e in rd.filter_map(Result::ok) {
        let proc_dir = e.path();
        if e.file_name().to_str().is_none_or(|n| !n.bytes().all(|b| b.is_ascii_digit()))
            || fs::metadata(&proc_dir).map(|m| m.uid()).ok() != Some(uid)
        {
            continue;
        }
        let Ok(raw) = fs::read(proc_dir.join("cmdline")) else { continue };
        let args: Vec<String> =
            raw.split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
        if args.first().and_then(|a| Path::new(a).file_name()).is_none_or(|n| n != program) {
            continue;
        }
        let cwd = fs::read_link(proc_dir.join("cwd")).ok();
        let dir = match scan_dir_arg(&args[1..], program == "s6-svscan") {
            Some(d) if d.is_relative() => cwd.map(|c| c.join(d)),
            Some(d) => Some(d),
            // s6-svscan без аргументу обслуговує поточну теку.
            None if program == "s6-svscan" => cwd,
            None => None,
        };
        if let Some(d) = dir
            && !dirs.contains(&d)
        {
            dirs.push(d);
        }
    }
    dirs
}

/// Тека служб на постійному диску в домашній теці (тека в /run зникне після
/// перезавантаження, тож для неї краще автозапуск).
fn persistent_supervisor_dir(program: &str) -> Option<PathBuf> {
    let home = paths::home();
    supervisor_dirs(program).into_iter().find(|d| d.starts_with(&home) && d.is_dir())
}

/// Систему завантажив systemd (як `sd_booted()`): самої програми systemctl для цього замало.
pub(super) fn systemd_booted() -> bool {
    Path::new("/run/systemd/system").is_dir()
}

/// Що підходить для цієї системи — від найкращого; автозапуск завжди останній.
pub fn available() -> Vec<Place> {
    let mut out = Vec::new();
    let fixed = |b: Backend| Place { backend: b, path: b.fixed_path().unwrap_or_default() };
    if systemd_booted() && quiet_ok("systemctl", &["--user", "show-environment"]) {
        out.push(fixed(Backend::Systemd));
    }
    if quiet_ok("dinitctl", &["--user", "list"]) {
        out.push(fixed(Backend::Dinit));
    }
    if find_in_path("rc-update").is_some()
        && std::env::var_os("XDG_RUNTIME_DIR").is_some_and(|d| Path::new(&d).join("openrc").is_dir())
    {
        out.push(fixed(Backend::Openrc));
    }
    if find_in_path("sv").is_some()
        && let Some(dir) = persistent_supervisor_dir("runsvdir")
    {
        out.push(Place { backend: Backend::Runit, path: dir.join(NAME) });
    }
    if find_in_path("s6-svc").is_some()
        && let Some(dir) = persistent_supervisor_dir("s6-svscan")
    {
        out.push(Place { backend: Backend::S6, path: dir.join(NAME) });
    }
    out.push(fixed(Backend::Autostart));
    out
}

/// Де служба вже встановлена (файли з маркером).
pub fn installed() -> Vec<Place> {
    let home = paths::home();
    let mut out: Vec<Place> = Backend::ALL
        .into_iter()
        .filter_map(|b| Some(Place { backend: b, path: b.fixed_path()? }))
        .filter(Place::ours)
        .collect();
    for (backend, program) in [(Backend::Runit, "runsvdir"), (Backend::S6, "s6-svscan")] {
        let mut dirs = supervisor_dirs(program);
        if backend == Backend::Runit {
            dirs.extend([home.join(".config/service"), home.join("service"), home.join(".local/service")]);
        }
        for d in dirs {
            let p = Place { backend, path: d.join(NAME) };
            if p.ours() && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn scan_dirs_from_cmdline() {
        assert_eq!(
            scan_dir_arg(&args(&["-P", "/home/u/.config/service", "log: ..."]), false),
            Some("/home/u/.config/service".into())
        );
        assert_eq!(scan_dir_arg(&args(&["/home/u/service"]), false), Some("/home/u/service".into()));
        assert_eq!(
            scan_dir_arg(&args(&["-t", "5000", "-d3", "/run/user/1000/s6"]), true),
            Some("/run/user/1000/s6".into())
        );
        assert_eq!(scan_dir_arg(&args(&["-c", "500"]), true), None);
    }
}
