//! Файли служб для кожної init-системи (з маркером `MARKER`) і їхній атомарний запис.

use std::path::Path;

use super::{Backend, MARKER, NAME};
use crate::paths::find_in_path;

/// Шлях у лапках для командного рядка служби, якщо в ньому є пробіли чи лапки.
fn quoted(path: &Path) -> String {
    let s = path.display().to_string();
    if s.chars().any(|c| c.is_whitespace() || c == '"' || c == '\\') {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s
    }
}

/// Шлях у лапках для sh.
fn sh_quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

/// Шлях для рядка Exec= у .desktop (специфікація Desktop Entry: зарезервовані
/// символи — у лапках, а \\ ще й екранується на рівні рядка).
fn desktop_exec(path: &Path) -> String {
    let s = path.display().to_string().replace('%', "%%");
    if !s.chars().any(|c| c.is_whitespace() || "\"'\\><~|&;$*?#()`".contains(c)) {
        return s;
    }
    let mut q = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '`' | '$' => {
                q.push('\\');
                q.push(c);
            }
            '\\' => q.push_str("\\\\\\\\"),
            _ => q.push(c),
        }
    }
    q.push('"');
    q
}

/// Опис служби (коментарі й Description) — мовою інтерфейсу на момент увімкнення.
fn about() -> &'static str {
    tr!(
        "OBS-Legacy: сповіщення про нові версії OBS, плагінів і самого OBS-Legacy",
        "OBS-Legacy: notifications about new versions of OBS, plugins and OBS-Legacy itself"
    )
}

pub fn render(backend: Backend, exe: &Path) -> String {
    let about = about();
    let manage = tr!(
        "Керувати: obs-legacy → Додатково → «Сповіщення про оновлення» або obs-legacy --notifier status|disable",
        "Manage: obs-legacy → More → “Update notifications” or obs-legacy --notifier status|disable"
    );
    let limit = |n: u32, minutes: u32, who: &str| {
        trf!(
            "Падає раз за разом — після {n} перезапусків за {minutes} хв {who} більше не перезапускає.",
            "Crashing again and again — after {n} restarts within {minutes} min {who} stops restarting it."
        )
    };
    match backend {
        Backend::Systemd => format!(
            "# {MARKER}\n# {about}.\n# {manage}\n[Unit]\nDescription={about}\n# {}\n\
             StartLimitIntervalSec=600\nStartLimitBurst=4\n\n[Service]\nType=simple\n\
             ExecStart={} --watch\nRestart=on-failure\nRestartSec=60\n\n[Install]\nWantedBy=default.target\n",
            limit(4, 10, "systemd"),
            quoted(exe)
        ),
        Backend::Dinit => format!(
            "# {MARKER}\n# {about}.\n# {manage}\n# ({} dinitctl --user status {NAME})\n# {}\n\
             type     = process\ncommand  = {} --watch\nrestart  = true\n# {}\n\
             restart-delay          = 10\nrestart-limit-interval = 300\nrestart-limit-count    = 3\n\
             log-type = buffer\n",
            tr!("або", "or"),
            tr!(
                "Служба користувача: її запускає dinit --user після входу, а не системний dinit.",
                "A user service: dinit --user starts it after login, not the system dinit."
            ),
            quoted(exe),
            limit(3, 5, "dinit")
        ),
        Backend::Openrc => format!(
            "#!{}\n# {MARKER}\n# {manage}\ndescription=\"{about}\"\nsupervisor=supervise-daemon\n\
             command={}\ncommand_args=\"--watch\"\n# {}\n\
             respawn_delay=60\nrespawn_max=3\nrespawn_period=600\n",
            find_in_path("openrc-run").map_or_else(|| "/sbin/openrc-run".into(), |p| p.display().to_string()),
            sh_quoted(exe),
            limit(3, 10, "supervise-daemon")
        ),
        Backend::Runit | Backend::S6 => {
            format!("#!/bin/sh\n# {MARKER}\n# {about}.\n# {manage}\nexec {} --watch 2>&1\n", sh_quoted(exe))
        }
        Backend::Autostart => {
            let exec = desktop_exec(exe);
            // Назва й опис — обома мовами: показує їх середовище робочого столу за своєю мовою.
            format!(
                "[Desktop Entry]\n# {MARKER}\n# {manage}\nType=Application\n\
                 Name=OBS-Legacy: update notifications\nName[uk]=OBS-Legacy: сповіщення про оновлення\n\
                 Comment=Checks for new OBS, plugin and OBS-Legacy versions every 12 hours\n\
                 Comment[uk]=Раз на 12 годин перевіряє нові версії OBS, плагінів і OBS-Legacy\n\
                 Exec={exec} --watch\nIcon=obs-legacy\nTerminal=false\nNoDisplay=true\n\
                 X-GNOME-Autostart-enabled=true\n"
            )
        }
    }
}

/// `finish` для runit і s6: після падіння (а не зупинки) — хвилина паузи перед
/// перезапуском, інакше runsv і s6-supervise перезапускали б щосекунди. $1 — код
/// виходу ./run; зупинку сигналом runit передає як -1, s6 — як 256.
fn render_finish() -> String {
    format!(
        "#!/bin/sh\n# {MARKER}\n# {}\n\
         [ \"$1\" -gt 0 ] 2>/dev/null && [ \"$1\" -lt 256 ] && sleep 60\nexit 0\n",
        tr!(
            "Фонова перевірка впала (а не зупинена) — зачекати хвилину перед перезапуском.",
            "The background check crashed (rather than was stopped) — wait a minute before restarting."
        )
    )
}

/// Файли теки служби runit чи s6: назва, вміст, права.
pub(super) fn supervised_files(backend: Backend, exe: &Path) -> Vec<(&'static str, String, u32)> {
    let mut files = vec![("run", render(backend, exe), 0o755), ("finish", render_finish(), 0o755)];
    if backend == Backend::S6 {
        // s6 дає finish 5 с, а далі вбиває: хвилинна пауза має встигнути.
        files.push(("timeout-finish", "70000\n".into(), 0o644));
    }
    files
}

/// Записати файл атомарно й надійно (див. `crate::atomic`).
pub(super) fn write_file(path: &Path, body: &str, mode: u32) -> Result<(), String> {
    crate::atomic::write(path, body.as_bytes(), mode)
        .map_err(|e| trf!("не вдалося записати {}: {e}", "couldn't write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    use super::*;
    use crate::service::detect::systemd_booted;

    #[test]
    fn unit_files() {
        let exe = Path::new("/home/u/.local/bin/obs-legacy");
        let systemd = render(Backend::Systemd, exe);
        assert!(systemd.contains("ExecStart=/home/u/.local/bin/obs-legacy --watch\n"));
        assert!(systemd.contains("WantedBy=default.target"));
        assert!(systemd.contains("StartLimitIntervalSec=600\nStartLimitBurst=4\n"));
        let dinit = render(Backend::Dinit, exe);
        assert!(dinit.contains("command  = /home/u/.local/bin/obs-legacy --watch\n"));
        assert!(dinit.contains("restart  = true"));
        assert!(dinit.contains("restart-delay          = 10\n"));
        assert!(dinit.contains("restart-limit-count    = 3\n"));
        let openrc = render(Backend::Openrc, exe);
        assert!(openrc.contains("command='/home/u/.local/bin/obs-legacy'\ncommand_args=\"--watch\""));
        assert!(openrc.contains("respawn_delay=60\nrespawn_max=3\nrespawn_period=600\n"));
        assert!(render(Backend::Runit, exe).contains("exec '/home/u/.local/bin/obs-legacy' --watch 2>&1"));
        let names = |b| supervised_files(b, exe).into_iter().map(|f| f.0).collect::<Vec<_>>();
        assert_eq!(names(Backend::Runit), ["run", "finish"]);
        assert_eq!(names(Backend::S6), ["run", "finish", "timeout-finish"]);
        let desktop = render(Backend::Autostart, exe);
        assert!(desktop.starts_with("[Desktop Entry]\n"));
        assert!(desktop.contains("Exec=/home/u/.local/bin/obs-legacy --watch\n"));
        for b in Backend::ALL {
            assert!(render(b, exe).contains(MARKER), "{b:?}");
            assert_eq!(Backend::from_name(b.name()), Some(b));
        }
    }

    #[test]
    fn autostart_entry_is_valid() {
        // desktop-file-validate є не всюди (пакет desktop-file-utils) — тоді лише формат вище.
        let Some(validator) = find_in_path("desktop-file-validate") else { return };
        let file = std::env::temp_dir().join(format!("{NAME}-{}.desktop", std::process::id()));
        fs::write(&file, render(Backend::Autostart, Path::new("/home/u/My Apps/obs-legacy"))).unwrap();
        let out = Command::new(validator).arg(&file).output().unwrap();
        let _ = fs::remove_file(&file);
        let report = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success() && !report.contains("error"), "{report}");
    }

    /// Справжній виконуваний файл: перевіряльники попереджають, якщо програми немає.
    fn real_exe() -> PathBuf {
        std::env::current_exe().unwrap()
    }

    #[test]
    fn shell_scripts_parse() {
        let exe = Path::new("/home/u/My Apps/it's/obs-legacy");
        let mut scripts = vec![render(Backend::Openrc, exe)];
        for b in [Backend::Runit, Backend::S6] {
            scripts.extend(supervised_files(b, exe).into_iter().filter(|f| f.2 == 0o755).map(|f| f.1));
        }
        for body in scripts {
            let out = Command::new("sh").args(["-n", "-c", &body]).output().unwrap();
            assert!(out.status.success(), "{body}\n{}", String::from_utf8_lossy(&out.stderr));
        }
    }

    #[test]
    fn finish_pauses_only_after_crash() {
        let script = render_finish().replace("sleep 60", "echo pause");
        let finish = |code: &str| {
            let out = Command::new("sh").args(["-c", &script, "finish", code, "0"]).output().unwrap();
            assert!(out.status.success());
            String::from_utf8_lossy(&out.stdout).contains("pause")
        };
        // Впала (код 1, паніка 101, програми немає 127) — пауза; зупинили сигналом чи вийшла сама — ні.
        assert!(finish("1") && finish("101") && finish("127"));
        assert!(!finish("0") && !finish("-1") && !finish("256"));
    }

    #[test]
    fn systemd_unit_is_valid() {
        // Лише там, де systemd справді працює (наприклад, у CI на Ubuntu): у контейнері без нього
        // (збирання пакета в Arch) systemd-analyze є, але падає на /run/systemd.
        if !systemd_booted() {
            return;
        }
        let Some(analyze) = find_in_path("systemd-analyze") else { return };
        let dir = std::env::temp_dir().join(format!("{NAME}-unit-{}", std::process::id()));
        let unit = dir.join(format!("{NAME}.service"));
        write_file(&unit, &render(Backend::Systemd, &real_exe()), 0o644).unwrap();
        let out = Command::new(analyze).arg("verify").arg(&unit).output().unwrap();
        let _ = fs::remove_dir_all(&dir);
        let report = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success() && !report.contains(NAME), "{report}");
    }

    #[test]
    fn paths_with_spaces_are_quoted() {
        let exe = Path::new("/home/u/My Apps/obs-legacy");
        assert!(render(Backend::Systemd, exe).contains("ExecStart=\"/home/u/My Apps/obs-legacy\" --watch"));
        assert!(render(Backend::Dinit, exe).contains("command  = \"/home/u/My Apps/obs-legacy\" --watch"));
        assert!(render(Backend::Runit, exe).contains("exec '/home/u/My Apps/obs-legacy' --watch"));
        assert!(render(Backend::Autostart, exe).contains("Exec=\"/home/u/My Apps/obs-legacy\" --watch"));
        assert_eq!(sh_quoted(Path::new("/a/it's")), r"'/a/it'\''s'");
    }
}
