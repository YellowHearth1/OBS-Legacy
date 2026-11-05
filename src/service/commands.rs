//! Команди `obs-legacy --notifier …`: стан, пробне сповіщення, увімкнення, перезапуск і вимкнення.
//! Друкують у форматі скрипта (`cli`), тож TUI показує їхні поради в картці результату.

use std::process::ExitCode;

use super::control::{activate, deactivate, stop_running, wait_running};
use super::detect::{available, installed};
use super::dinit::dinit_errors;
use super::{Backend, Place, exe_path, status};
use crate::cli;
use crate::notify;
use crate::paths::short_path;
use crate::updates;

/// «служба dinit» → «Служба dinit» (на початку речення).
fn upper_first(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// Чужий файл з нашою назвою: не перезаписуємо.
fn foreign(p: &Place) -> bool {
    p.marker_file().exists() && !p.ours()
}

pub(super) fn print_status() -> ExitCode {
    let s = status();
    cli::say(tr!("Сповіщення про оновлення", "Update notifications"));
    if s.installed.is_empty() {
        cli::warn(tr!("Вимкнено.", "Off."));
        let best = available().into_iter().next();
        if let Some(b) = best {
            cli::hint(&trf!(
                "Увімкнути: obs-legacy → Додатково → «Сповіщення про оновлення» або obs-legacy --notifier enable \
                 ({} підходить для цієї системи).",
                "To turn on: obs-legacy → More → “Update notifications” or obs-legacy --notifier enable \
                 ({} suits this system).",
                b.backend.label()
            ));
        }
    }
    for p in &s.installed {
        cli::ok(&trf!("Увімкнено: {} ({}).", "On: {} ({}).", p.backend.label(), p.shown()));
        if p.backend == Backend::Dinit
            && let Some(errors) = p.path.parent().and_then(dinit_errors)
        {
            if errors.is_empty() {
                cli::ok(tr!(
                    "Опис служби dinit перевірено (dinit-check): помилок немає.",
                    "The dinit service description is checked (dinit-check): no errors."
                ));
            } else {
                cli::fail(&trf!(
                    "dinit-check знайшов помилки в описі служби: {}",
                    "dinit-check found errors in the service description: {}",
                    errors.join("; ")
                ));
                cli::hint(tr!(
                    "Зламаний опис служби в boot.d не дає dinit --user запуститися після входу. Вимкни й увімкни \
                     сповіщення знову: obs-legacy --notifier disable, потім obs-legacy --notifier enable",
                    "A broken service description in boot.d keeps dinit --user from starting after login. Turn the \
                     notifications off and on again: obs-legacy --notifier disable, then obs-legacy --notifier enable"
                ));
            }
        }
    }
    match s.running {
        Some(pid) => {
            cli::ok(&trf!("Фонова перевірка працює (PID {pid}).", "The background check is running (PID {pid})."))
        }
        None if s.enabled() => {
            cli::warn(tr!("Фонова перевірка зараз не працює.", "The background check isn't running now."));
            cli::hint(tr!(
                "Вона запуститься після наступного входу в систему; одразу — obs-legacy --notifier restart.",
                "It starts after the next login; right now — obs-legacy --notifier restart."
            ));
        }
        None => {}
    }
    if let Some(line) = s.last_line() {
        cli::info(&line);
    }
    cli::info(&trf!("Журнал: {}", "Log: {}", short_path(&updates::log_path())));
    ExitCode::SUCCESS
}

fn send_test() -> Result<String, String> {
    let session = notify::find_session().ok_or(tr!(
        "графічної сесії не знайдено (немає WAYLAND_DISPLAY чи DISPLAY)",
        "no graphical session found (no WAYLAND_DISPLAY or DISPLAY)"
    ))?;
    notify::send(
        &session,
        &notify::Message {
            title: tr!("OBS-Legacy: сповіщення працюють", "OBS-Legacy: notifications work"),
            body: tr!(
                "Так виглядатимуть новини про нові версії OBS, плагінів і самого OBS-Legacy.",
                "This is how news about new versions of OBS, plugins and OBS-Legacy itself will look."
            ),
            urgent: false,
            open: None,
        },
    )?;
    Ok(session.describe())
}

pub(super) fn test_cmd() -> ExitCode {
    cli::say(tr!("Пробне сповіщення", "Test notification"));
    match send_test() {
        Ok(desc) => {
            cli::ok(&trf!(
                "Надіслано ({desc}). Воно має з'явитися на робочому столі.",
                "Sent ({desc}). It should appear on the desktop."
            ));
            ExitCode::SUCCESS
        }
        Err(e) => {
            cli::fail(&trf!("Не вдалося показати сповіщення: {e}.", "Couldn't show the notification: {e}."));
            cli::hint(tr!(
                "Потрібна служба сповіщень робочого столу (у GNOME, KDE, Xfce вона вбудована; у sway, Hyprland — \
                 mako, dunst чи swaync) і notify-send з пакета libnotify: sudo pacman -S --needed libnotify.",
                "You need a desktop notification service (GNOME, KDE and Xfce have one built in; on sway or Hyprland \
                 — mako, dunst or swaync) and notify-send from the libnotify package: sudo pacman -S --needed libnotify."
            ));
            ExitCode::FAILURE
        }
    }
}

pub(super) fn enable_cmd(choice: Option<Backend>, again_only: bool) -> ExitCode {
    // SAFETY: geteuid не має передумов.
    if unsafe { libc::geteuid() } == 0 {
        cli::fail(tr!(
            "Сповіщення вмикаються для звичайного користувача, а не для root.",
            "Notifications are turned on for a regular user, not for root."
        ));
        cli::hint(tr!(
            "Запусти без sudo: obs-legacy --notifier enable",
            "Run it without sudo: obs-legacy --notifier enable"
        ));
        return ExitCode::FAILURE;
    }
    let exe = exe_path();
    let installed = installed();
    if again_only {
        // install.sh --update: перезапустити з новою програмою, якщо ввімкнено.
        let Some(place) = installed.first() else { return ExitCode::SUCCESS };
        return match activate(place, &exe, true) {
            Ok(()) => {
                cli::ok(&trf!(
                    "Фонову перевірку оновлень перезапущено ({}).",
                    "The background update check was restarted ({}).",
                    place.backend.label()
                ));
                ExitCode::SUCCESS
            }
            Err(e) => {
                cli::warn(&trf!(
                    "Не вдалося перезапустити фонову перевірку ({}): {e}",
                    "Couldn't restart the background check ({}): {e}",
                    place.backend.label()
                ));
                ExitCode::SUCCESS
            }
        };
    }
    cli::say(tr!("Вмикаю сповіщення про оновлення", "Turning on update notifications"));
    let avail = available();
    let target = match choice {
        Some(b) => match avail.iter().find(|p| p.backend == b) {
            Some(p) => p.clone(),
            None => {
                cli::fail(&trf!(
                    "{} для користувача тут не працює.",
                    "{} isn't available to the user here.",
                    upper_first(b.label())
                ));
                let names: Vec<&str> = avail.iter().map(|p| p.backend.name()).collect();
                cli::hint(&trf!(
                    "Що працює на цій системі: {}. Без назви програма вибере сама: obs-legacy --notifier enable",
                    "What works on this system: {}. Without a name the program picks one itself: obs-legacy --notifier enable",
                    names.join(", ")
                ));
                return ExitCode::FAILURE;
            }
        },
        None => avail[0].clone(),
    };
    if foreign(&target) {
        cli::fail(&trf!(
            "{} уже існує і створений не OBS-Legacy — не чіпаю.",
            "{} already exists and wasn't created by OBS-Legacy — leaving it alone.",
            target.shown()
        ));
        cli::hint(tr!(
            "Перейменуй або прибери цей файл і спробуй ще раз.",
            "Rename or remove this file and try again."
        ));
        return ExitCode::FAILURE;
    }
    // Одна служба: інші, якщо були, прибрати (наприклад, після зміни init-системи).
    for p in installed.iter().filter(|p| **p != target) {
        match deactivate(p) {
            Ok(()) => cli::ok(&trf!(
                "Прибрано попередню: {} ({}).",
                "Removed the previous one: {} ({}).",
                p.backend.label(),
                p.shown()
            )),
            Err(e) => cli::warn(&trf!("Не вдалося прибрати {}: {e}", "Couldn't remove {}: {e}", p.shown())),
        }
    }
    let again = installed.contains(&target);
    if let Err(e) = activate(&target, &exe, again) {
        // Нова служба не запрацювала — не лишати її напівувімкненою (файл без запуску, запис у boot.d).
        if !again && target.ours() {
            let _ = deactivate(&target);
        }
        cli::fail(&trf!("Не вдалося ввімкнути {}: {e}", "Couldn't turn on the {}: {e}", target.backend.label()));
        if target.backend != Backend::Autostart {
            cli::hint(tr!(
                "Спробуй автозапуск сесії замість служби: obs-legacy --notifier enable autostart",
                "Try the session autostart instead of a service: obs-legacy --notifier enable autostart"
            ));
        }
        return ExitCode::FAILURE;
    }
    cli::ok(&format!("{}: {}", upper_first(target.backend.label()), target.shown()));
    match wait_running(10) {
        Some(pid) => cli::ok(&trf!(
            "Фонова перевірка працює (PID {pid}): нові версії — раз на 12 годин.",
            "The background check is running (PID {pid}): new versions — every 12 hours."
        )),
        None => cli::warn(tr!(
            "Служба ввімкнена, але фонова перевірка ще не відповіла.",
            "The service is on, but the background check hasn't answered yet."
        )),
    }
    match send_test() {
        Ok(_) => cli::ok(tr!(
            "Надіслав пробне сповіщення — воно має з'явитися на робочому столі.",
            "Sent a test notification — it should appear on the desktop."
        )),
        Err(e) => {
            cli::warn(&trf!("Пробне сповіщення не показано: {e}.", "The test notification wasn't shown: {e}."));
            cli::hint(tr!(
                "Потрібна служба сповіщень робочого столу (у GNOME, KDE, Xfce вбудована; у sway, Hyprland — mako, \
                 dunst чи swaync) і пакет libnotify.",
                "You need a desktop notification service (built into GNOME, KDE and Xfce; on sway or Hyprland — mako, \
                 dunst or swaync) and the libnotify package."
            ));
        }
    }
    if target.backend == Backend::Autostart {
        cli::warn(tr!(
            "Служб користувача init-система тут не запускає, тож перевірку запускає автозапуск сесії.",
            "The init system here doesn't run user services, so the session autostart runs the check."
        ));
        cli::hint(tr!(
            "Якщо твій віконний менеджер не виконує ~/.config/autostart (i3, sway, Hyprland…), додай у його \
             конфіг запуск: obs-legacy --watch",
            "If your window manager doesn't run ~/.config/autostart (i3, sway, Hyprland…), add this to its \
             config: obs-legacy --watch"
        ));
    }
    cli::info(tr!(
        "Вимкнути: obs-legacy → Додатково → «Сповіщення про оновлення» або obs-legacy --notifier disable.",
        "To turn off: obs-legacy → More → “Update notifications” or obs-legacy --notifier disable."
    ));
    ExitCode::SUCCESS
}

pub(super) fn disable_cmd() -> ExitCode {
    cli::say(tr!("Вимикаю сповіщення про оновлення", "Turning off update notifications"));
    let installed = installed();
    let mut code = ExitCode::SUCCESS;
    for p in &installed {
        match deactivate(p) {
            Ok(()) => cli::ok(&trf!("Прибрано: {} ({}).", "Removed: {} ({}).", p.backend.label(), p.shown())),
            Err(e) => {
                cli::fail(&trf!("Не вдалося прибрати {}: {e}", "Couldn't remove {}: {e}", p.shown()));
                code = ExitCode::FAILURE;
            }
        }
    }
    // Фонова перевірка без служби (запущена вручну чи автозапуском) — теж зупинити.
    stop_running();
    if installed.is_empty() {
        cli::ok(tr!("Сповіщення й так вимкнено.", "Notifications are already off."));
    } else {
        cli::ok(tr!("Сповіщень про оновлення більше не буде.", "No more update notifications."));
    }
    code
}
