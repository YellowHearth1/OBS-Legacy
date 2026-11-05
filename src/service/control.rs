//! Увімкнути, перезапустити чи вимкнути службу в її init-системі й керувати фоновою перевіркою, яку
//! вона запускає.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use super::dinit::{dinit_boot_links, dinit_check_body, dinit_load};
use super::files::{render, supervised_files, write_file};
use super::{Backend, NAME, Place, run};
use crate::desktop;
use crate::updates;

/// Дочекатися, поки фонова перевірка запуститься.
pub(super) fn wait_running(secs: u64) -> Option<u32> {
    let start = Instant::now();
    loop {
        if let Some(pid) = updates::running_pid() {
            return Some(pid);
        }
        if start.elapsed() > Duration::from_secs(secs) {
            return None;
        }
        thread::sleep(Duration::from_millis(250));
    }
}

/// Запустити `--watch` окремо від термінала (для автозапуску — одразу, без
/// повторного входу в сесію).
fn spawn_watch(exe: &Path) -> Result<(), String> {
    desktop::spawn_detached(Command::new(exe).arg("--watch"))
        .map_err(|e| trf!("не вдалося запустити {} --watch: {e}", "couldn't start {} --watch: {e}", exe.display()))
}

/// Зупинити фонову перевірку, якщо вона працює (і це справді наш `--watch`).
pub(super) fn stop_running() {
    let Some(pid) = updates::running_pid() else { return };
    let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    if !cmdline.split(|b| *b == 0).any(|a| a == b"--watch") {
        return;
    }
    // SAFETY: kill лише надсилає сигнал процесу, PID якого записала фонова перевірка.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    let start = Instant::now();
    while updates::running_pid() == Some(pid) && start.elapsed() < Duration::from_secs(5) {
        thread::sleep(Duration::from_millis(100));
    }
}

/// Записати файл служби й запустити її. `again` — служба вже була: перезапуск.
pub(super) fn activate(place: &Place, exe: &Path, again: bool) -> Result<(), String> {
    let p = place.path.as_path();
    match place.backend {
        Backend::Systemd => {
            write_file(p, &render(Backend::Systemd, exe), 0o644)?;
            run("systemctl", &["--user", "daemon-reload"])?;
            let unit = format!("{NAME}.service");
            if again {
                run("systemctl", &["--user", "enable", &unit])?;
                run("systemctl", &["--user", "restart", &unit])?;
            } else {
                run("systemctl", &["--user", "enable", "--now", &unit])?;
            }
        }
        Backend::Dinit => {
            let body = render(Backend::Dinit, exe);
            dinit_check_body(&body)?;
            // Опис, який dinit уже прийняв, — повернути, якщо нового він не прийме: на старий
            // посилається boot.d, і зламаний опис там не дав би dinit --user запуститися.
            let previous = if again { fs::read_to_string(p).ok() } else { None };
            write_file(p, &body, 0o644)?;
            if let Err(e) = dinit_load(again) {
                if let Some(old) = previous {
                    let _ = write_file(p, &old, 0o644);
                    let _ = run("dinitctl", &["--user", "reload", NAME]);
                    let _ = run("dinitctl", &["--user", "start", NAME]);
                }
                return Err(e);
            }
            let action =
                if again && run("dinitctl", &["--user", "is-started", NAME]).is_ok() { "restart" } else { "start" };
            run("dinitctl", &["--user", action, NAME])?;
        }
        Backend::Openrc => {
            write_file(p, &render(Backend::Openrc, exe), 0o755)?;
            let _ = run("rc-update", &["--user", "add", NAME, "default"]);
            let action = if again { "restart" } else { "start" };
            run("rc-service", &["--user", NAME, action])?;
        }
        Backend::Runit | Backend::S6 => {
            let files = supervised_files(place.backend, exe);
            if again && p.is_dir() {
                for (name, body, mode) in &files {
                    write_file(&p.join(name), body, *mode)?;
                }
                if place.backend == Backend::Runit {
                    run("sv", &["restart", &p.display().to_string()])?;
                } else {
                    run("s6-svc", &["-r", &p.display().to_string()])?;
                }
            } else {
                // Тека з'являється одразу готовою: runsvdir і s6-svscan не беруть теки з крапкою.
                let parent = p.parent().ok_or(tr!("без теки", "no directory"))?;
                let tmp = parent.join(format!(".{NAME}.new"));
                let _ = fs::remove_dir_all(&tmp);
                for (name, body, mode) in &files {
                    write_file(&tmp.join(name), body, *mode)?;
                }
                fs::rename(&tmp, p)
                    .map_err(|e| trf!("не вдалося створити {}: {e}", "couldn't create {}: {e}", p.display()))?;
                if place.backend == Backend::S6 {
                    run("s6-svscanctl", &["-a", &parent.display().to_string()])?;
                }
            }
        }
        Backend::Autostart => {
            write_file(p, &render(Backend::Autostart, exe), 0o644)?;
            stop_running();
            spawn_watch(exe)?;
        }
    }
    Ok(())
}

/// Зупинити й прибрати службу.
pub(super) fn deactivate(place: &Place) -> Result<(), String> {
    let p = place.path.as_path();
    match place.backend {
        Backend::Systemd => {
            let _ = run("systemctl", &["--user", "disable", "--now", &format!("{NAME}.service")]);
            fs::remove_file(p).map_err(|e| e.to_string())?;
            let _ = run("systemctl", &["--user", "daemon-reload"]);
        }
        Backend::Dinit => {
            let _ = run("dinitctl", &["--user", "disable", NAME]);
            let _ = run("dinitctl", &["--user", "stop", NAME]);
            let _ = run("dinitctl", &["--user", "unload", NAME]);
            // Якщо dinit зараз не працює, disable посилання не прибере — прибрати самому, раніше
            // за опис, щоб не лишити запису без опису. dinit шукає службу з boot.d за назвою
            // запису, тож куди веде посилання — байдуже.
            for link in dinit_boot_links(p) {
                if fs::symlink_metadata(&link).is_ok() {
                    fs::remove_file(&link)
                        .map_err(|e| trf!("не вдалося прибрати {}: {e}", "couldn't remove {}: {e}", link.display()))?;
                }
            }
            fs::remove_file(p).map_err(|e| e.to_string())?;
        }
        Backend::Openrc => {
            let _ = run("rc-service", &["--user", NAME, "stop"]);
            let _ = run("rc-update", &["--user", "del", NAME, "default"]);
            fs::remove_file(p).map_err(|e| e.to_string())?;
        }
        Backend::Runit => {
            let dir = p.display().to_string();
            let _ = run("sv", &["down", &dir]);
            let _ = run("sv", &["exit", &dir]);
            fs::remove_dir_all(p).map_err(|e| e.to_string())?;
        }
        Backend::S6 => {
            let dir = p.display().to_string();
            let _ = run("s6-svc", &["-dx", &dir]);
            fs::remove_dir_all(p).map_err(|e| e.to_string())?;
            if let Some(parent) = p.parent() {
                let _ = run("s6-svscanctl", &["-an", &parent.display().to_string()]);
            }
        }
        Backend::Autostart => {
            fs::remove_file(p).map_err(|e| e.to_string())?;
            // Запущену автозапуском перевірку ніхто інший не зупинить.
            stop_running();
        }
    }
    Ok(())
}
