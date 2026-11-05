//! dinit — найкрихкіша з init-систем: зламаний опис служби, на яку посилається boot.d, не дає
//! `dinit --user` запуститися взагалі. Тут перевірка опису `dinit-check` до запису, записи в boot.d і
//! завантаження опису в dinit.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::files::write_file;
use super::{NAME, run};
use crate::paths::{self, find_in_path};

/// Помилки в описі служби dinit за `dinit-check` (у старих версіях — `dinitcheck`).
/// None — перевіряльника немає. Попередження (скажімо, «не знайдено програму»)
/// помилками не вважаються: такий опис dinit однаково прочитає.
pub(super) fn dinit_errors(dir: &Path) -> Option<Vec<String>> {
    let checker = ["dinit-check", "dinitcheck"].into_iter().find(|p| find_in_path(p).is_some())?;
    let (Ok(text) | Err(text)) = run(checker, &["-d", &dir.display().to_string(), NAME]);
    Some(text.lines().filter(|l| l.starts_with("Service '") && !l.contains("warning:")).map(str::to_string).collect())
}

/// Перевірити опис служби dinit до того, як він ляже в ~/.config/dinit.d (у
/// тимчасовій теці: у нашого опису немає залежностей).
pub(super) fn dinit_check_body(body: &str) -> Result<(), String> {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        ".{NAME}-check-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    write_file(&dir.join(NAME), body, 0o644)?;
    let errors = dinit_errors(&dir);
    let _ = fs::remove_dir_all(&dir);
    match errors {
        Some(e) if !e.is_empty() => Err(trf!(
            "dinit-check знайшов помилки в описі служби: {}",
            "dinit-check found errors in the service description: {}",
            e.join("; ")
        )),
        _ => Ok(()),
    }
}

/// Записи служби в boot.d (так їх роблять `dinitctl enable`, userspawn і turnstile:
/// ~/.config/dinit.d/boot.d; і поруч з нашим файлом, якщо XDG_CONFIG_HOME інший).
pub(super) fn dinit_boot_links(file: &Path) -> Vec<PathBuf> {
    let mut links = vec![file.with_file_name("boot.d").join(NAME)];
    let home = paths::home().join(".config/dinit.d/boot.d").join(NAME);
    if !links.contains(&home) {
        links.push(home);
    }
    links
}

/// Завантажити опис у dinit і додати службу до boot. dinit спершу читає опис,
/// тож зламаного до boot.d не додасть.
pub(super) fn dinit_load(again: bool) -> Result<(), String> {
    // Опис служби вже завантажено — перечитати (якщо dinit не дає — спершу зупинити).
    if again && run("dinitctl", &["--user", "reload", NAME]).is_err() {
        let _ = run("dinitctl", &["--user", "stop", NAME]);
        run("dinitctl", &["--user", "reload", NAME])?;
    }
    // Додає службу до boot (посилання в boot.d) і запускає; повторно — «already enabled».
    match run("dinitctl", &["--user", "enable", NAME]) {
        Err(e) if !e.contains("already enabled") => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::service::Backend;
    use crate::service::files::render;

    /// Справжній виконуваний файл: перевіряльники попереджають, якщо програми немає.
    fn real_exe() -> PathBuf {
        std::env::current_exe().unwrap()
    }

    #[test]
    fn dinit_description_is_valid() {
        // dinit-check є лише там, де стоїть dinit; без нього перевіряє сам dinit під час enable.
        if !["dinit-check", "dinitcheck"].iter().any(|p| find_in_path(p).is_some()) {
            return;
        }
        assert_eq!(dinit_check_body(&render(Backend::Dinit, &real_exe())), Ok(()));
        let broken = "type = process\ncommand = /bin/true\nrestart = maybe\n";
        assert!(dinit_check_body(broken).unwrap_err().contains("restart"));
    }
}
