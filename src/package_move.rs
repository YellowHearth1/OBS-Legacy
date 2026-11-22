//! `--use-package`: програма OBS-Legacy з пакета pacman переймає роботу в копії, яку install.sh поставив у
//! домашню теку (`~/.local/bin/obs-legacy`). pacman у домашні теки не пише ніколи, тож без цього стара копія
//! лишалася б першою в PATH, її ярлик у `~/.local/share/applications` затуляв би ярлик пакета, а служба
//! сповіщень і далі запускала б її. Порядок: спершу служба переходить на програму пакета, і лише тоді
//! прибираються стара програма, її ярлики, значок і позначки install.sh. Налаштування, набори пакетів, звіти й
//! розпакований скрипт лишаються. Запускають її obs-legacy.install після встановлення пакета, install.sh, Мацаки й людина.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::{cli, paths, service};

/// Позначка у файлах ярликів, які ставить install.sh (там — `MARKER`).
const INSTALL_SH_MARKER: &str = "Managed by OBS-Legacy install.sh";
const DESKTOP_ID: &str = "obs-legacy.desktop";

/// Копія, яку поставив install.sh: програма й файли, що прибираються разом з нею.
#[derive(Debug, PartialEq, Eq)]
pub struct HomeInstall {
    pub program: PathBuf,
    pub files: Vec<PathBuf>,
}

/// Чи стоїть копія install.sh: програма в `<home>/.local/bin` і позначка install.sh про те, звідки її
/// поставлено (`source-dir` чи `release-install` у теці даних програми). Ярлики — лише з позначкою install.sh.
pub fn find(home: &Path, data_home: &Path, desktop: Option<&Path>) -> Option<HomeInstall> {
    let program = home.join(".local/bin/obs-legacy");
    let state = data_home.join("obs-legacy");
    let marks: Vec<PathBuf> =
        ["source-dir", "release-install"].iter().map(|m| state.join(m)).filter(|p| p.is_file()).collect();
    if !program.is_file() || marks.is_empty() {
        return None;
    }
    let mut files = Vec::new();
    let entries = [Some(data_home.join("applications").join(DESKTOP_ID)), desktop.map(|d| d.join(DESKTOP_ID))];
    for entry in entries.into_iter().flatten() {
        if std::fs::read_to_string(&entry).is_ok_and(|text| text.contains(INSTALL_SH_MARKER)) {
            files.push(entry);
        }
    }
    let icon = data_home.join("icons/hicolor/scalable/apps/obs-legacy.svg");
    if icon.is_file() {
        files.push(icon);
    }
    files.extend(marks);
    Some(HomeInstall { program, files })
}

/// Тека робочого столу (`xdg-user-dir DESKTOP`), якщо її можна дізнатися.
fn desktop_dir() -> Option<PathBuf> {
    let out = std::process::Command::new("xdg-user-dir").arg("DESKTOP").output().ok()?;
    let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!dir.is_empty() && Path::new(&dir).is_dir()).then(|| PathBuf::from(dir))
}

pub fn main() -> ExitCode {
    // SAFETY: geteuid не має передумов.
    if unsafe { libc::geteuid() } == 0 {
        cli::fail(tr!(
            "Перехід на пакет робиться від імені користувача, а не root.",
            "Switching to the package is done as the user, not as root."
        ));
        return ExitCode::FAILURE;
    }
    let exe = service::exe_path();
    if !exe.starts_with("/usr/") {
        cli::fail(&trf!(
            "Цю команду запускає програма з пакета, а зараз працює {}.",
            "This command is run by the program from the package, but this is {}.",
            exe.display()
        ));
        cli::hint(tr!(
            "Спершу встанови пакет obs-legacy, потім: /usr/bin/obs-legacy --use-package",
            "Install the obs-legacy package first, then: /usr/bin/obs-legacy --use-package"
        ));
        return ExitCode::FAILURE;
    }
    let Some(old) = find(&paths::home(), &paths::data_home(), desktop_dir().as_deref()) else {
        cli::ok(tr!(
            "Копії install.sh у домашній теці немає — уже працює програма з пакета.",
            "There is no install.sh copy in the home folder — the program from the package already runs."
        ));
        return ExitCode::SUCCESS;
    };
    // Служба сповіщень — першою: поки вона запускає стару програму, видаляти її не можна.
    if service::status().enabled() {
        let restarted = service::main(&["restart".to_string()]);
        if restarted != ExitCode::SUCCESS || service_still_uses(&old.program) {
            cli::fail(tr!(
                "Службу сповіщень не вдалося перевести на програму з пакета — стару копію не чіпаю.",
                "Could not switch the notification service to the program from the package — the old copy stays."
            ));
            cli::hint(tr!(
                "Подивись obs-legacy --notifier status; потім знову /usr/bin/obs-legacy --use-package.",
                "See obs-legacy --notifier status; then run /usr/bin/obs-legacy --use-package again."
            ));
            return ExitCode::FAILURE;
        }
    }
    let mut left = Vec::new();
    for file in std::iter::once(&old.program).chain(&old.files) {
        if let Err(e) = std::fs::remove_file(file) {
            left.push(format!("{} ({e})", file.display()));
        }
    }
    let apps = paths::data_home().join("applications");
    let _ = std::process::Command::new("update-desktop-database")
        .args(["-q", &apps.to_string_lossy()])
        .stderr(std::process::Stdio::null())
        .status();
    if !left.is_empty() {
        cli::warn(&trf!("Не вдалося прибрати: {}", "Could not remove: {}", left.join(", ")));
    }
    cli::ok(&trf!(
        "Тепер працює OBS-Legacy з пакета ({}): стару копію {} прибрано разом з її ярликом і значком; налаштування й набори пакетів лишилися.",
        "OBS-Legacy now runs from the package ({}): the old copy {} is removed together with its shortcut and icon; the settings and package sets stay.",
        exe.display(),
        old.program.display()
    ));
    ExitCode::SUCCESS
}

/// Чи файли служби досі називають стару програму.
fn service_still_uses(program: &Path) -> bool {
    let needle = program.to_string_lossy().into_owned();
    let mentions = |f: &Path| std::fs::read_to_string(f).is_ok_and(|text| text.contains(&needle));
    // dinit, OpenRC, autostart — файл; runit і s6 — тека з файлами (run тощо)
    service::status().installed.iter().any(|place| {
        mentions(&place.path)
            || std::fs::read_dir(&place.path).into_iter().flatten().flatten().any(|e| mentions(&e.path()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_only_what_install_sh_put_there() {
        let root = std::env::temp_dir().join(format!("obs-legacy-move-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (home, data, desk) = (root.join("home"), root.join("home/.local/share"), root.join("home/Desktop"));
        for dir in [home.join(".local/bin"), data.join("obs-legacy"), data.join("applications"), desk.clone()] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(home.join(".local/bin/obs-legacy"), "elf").unwrap();
        // без позначки install.sh — не наша копія
        assert_eq!(find(&home, &data, Some(&desk)), None);
        std::fs::write(data.join("obs-legacy/release-install"), "v0.0.6\n").unwrap();
        std::fs::write(data.join("applications").join(DESKTOP_ID), format!("# {INSTALL_SH_MARKER}\n")).unwrap();
        // чужий ярлик на столі (без позначки) не чіпаємо
        std::fs::write(desk.join(DESKTOP_ID), "[Desktop Entry]\n").unwrap();
        let found = find(&home, &data, Some(&desk)).expect("install.sh copy");
        assert_eq!(found.program, home.join(".local/bin/obs-legacy"));
        assert_eq!(
            found.files,
            vec![data.join("applications").join(DESKTOP_ID), data.join("obs-legacy/release-install")]
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
