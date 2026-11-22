//! Автоматичне оновлення пакетів OBS-Legacy разом із системою: добудовувач
//! `/usr/lib/obs-legacy/auto-update` з пакета obs-legacy (його запускає хук pacman від root;
//! docs/ARCHITECTURE.md → «Автоматичне оновлення разом із системою»). Тут — те, що про нього знає
//! програма: чи він стоїть (тоді фонова перевірка не збирає сама, коли оновлення системи зачепило
//! бібліотеки, — це робить він) і сповіщення, які він показує через `obs-legacy --auto-update-notice`.

use std::path::Path;
use std::process::ExitCode;

use super::notices::Notice;
use crate::{cli, notify, service};

/// Добудовувач з пакета obs-legacy.
pub const HELPER: &str = "/usr/lib/obs-legacy/auto-update";
/// Його журнал (читати може кожен).
pub const LOG: &str = "/var/log/obs-legacy/auto-update.log";

/// Чи стоїть добудовувач: тоді пакети, яким після оновлення системи бракує бібліотек, лагодить і
/// ставить він — у тій самій транзакції pacman, що й оновлення.
pub fn installed() -> bool {
    Path::new(HELPER).is_file()
}

/// «obs-studio-legacy-580xx-33.0.3-14-x86_64.pkg.tar.zst» → «obs-studio-legacy-580xx 33.0.3-14»:
/// назва пакета не має дефіса лише в останніх трьох полях (версія, випуск, архітектура).
fn package_label(file: &str) -> String {
    let base = file.rsplit('/').next().unwrap_or(file);
    let stem = base.split(".pkg.tar").next().unwrap_or(base);
    let mut parts = stem.rsplitn(4, '-');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(_arch), Some(rel), Some(ver), Some(name)) if !name.is_empty() => format!("{name} {ver}-{rel}"),
        _ => stem.to_string(),
    }
}

/// Сповіщення добудовувача: `started` — почав перезбирати, `ok` — поставив (далі — файли пакетів),
/// `failed` — не вдалося. Невідомий вид — None.
pub fn notice(kind: &str, files: &[String]) -> Option<Notice> {
    let (title, body, urgent) = match kind {
        "started" => (
            tr!("OBS-Legacy перезбирає пакети", "OBS-Legacy is rebuilding its packages").to_string(),
            tr!(
                "Оновлення системи змінило бібліотеки, з якими зібрано OBS-Legacy. Потрібні пакети перезбираються \
                 у фоні й встановляться самі; OBS тим часом працює як і раніше.",
                "The system upgrade changed libraries OBS-Legacy is built with. The packages that need it are \
                 being rebuilt in the background and will install themselves; OBS works as before meanwhile."
            )
            .to_string(),
            false,
        ),
        "ok" => {
            let list: Vec<String> = files.iter().flat_map(|f| f.split_whitespace()).map(package_label).collect();
            (
                tr!("OBS-Legacy оновлено разом із системою", "OBS-Legacy is updated together with the system")
                    .to_string(),
                trf!(
                    "Встановлено: {}. Якщо OBS відкритий, перезапусти його.",
                    "Installed: {}. If OBS is open, restart it.",
                    list.join(", ")
                ),
                false,
            )
        }
        "failed" => (
            tr!("OBS-Legacy не зміг оновитися сам", "OBS-Legacy could not update itself").to_string(),
            trf!(
                "Після оновлення системи пакети OBS-Legacy не вдалося перезібрати й поставити самостійно. OBS \
                 працює на збережених копіях бібліотек. Відкрий OBS-Legacy → «Встановити / оновити OBS» \
                 (журнал: {LOG}).",
                "After the system upgrade the OBS-Legacy packages could not be rebuilt and installed on their \
                 own. OBS runs on kept library copies. Open OBS-Legacy → “Install / update OBS” (log: {LOG})."
            ),
            true,
        ),
        _ => return None,
    };
    Some(Notice { items: vec![("auto-update".into(), kind.into())], title, body, urgent })
}

/// `obs-legacy --auto-update-notice <started|ok|failed> [файли пакетів]` — його викликає добудовувач
/// від імені користувача. Без графічної сесії — лише попередження: збій сповіщення не збій оновлення.
pub fn notice_main(args: &[String]) -> ExitCode {
    let Some(n) = args.first().and_then(|kind| notice(kind, &args[1..])) else {
        cli::fail(tr!(
            "Потрібно: --auto-update-notice started|ok|failed [файли пакетів]",
            "Usage: --auto-update-notice started|ok|failed [package files]"
        ));
        return ExitCode::from(2);
    };
    let Some(session) = notify::find_session() else {
        cli::warn(tr!(
            "Графічної сесії не знайдено — сповіщення не показано.",
            "No graphical session found — the notification wasn't shown."
        ));
        return ExitCode::SUCCESS;
    };
    let exe = service::exe_path();
    if let Err(e) = notify::send(&session, &n.message(Some(&exe))) {
        cli::warn(&trf!("Не вдалося показати сповіщення: {e}", "Couldn't show the notification: {e}"));
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_files_become_names_and_versions() {
        assert_eq!(
            package_label("/home/u/.local/share/x/obs-studio-legacy-580xx-33.0.3-14-x86_64.pkg.tar.zst"),
            "obs-studio-legacy-580xx 33.0.3-14"
        );
        assert_eq!(package_label("ffmpeg-legacy-580xx-2:9.1-1.3-x86_64.pkg.tar.zst"), "ffmpeg-legacy-580xx 2:9.1-1.3");
        assert_eq!(package_label("odd.pkg.tar.zst"), "odd");
    }

    #[test]
    fn notices_for_each_outcome() {
        let ok = notice("ok", &["a-legacy-580xx-1-2-x86_64.pkg.tar.zst b-legacy-580xx-3-4-any.pkg.tar.zst".into()])
            .expect("ok");
        assert!(ok.body.contains("a-legacy-580xx 1-2, b-legacy-580xx 3-4"), "{}", ok.body);
        assert!(!ok.urgent);
        let failed = notice("failed", &[]).expect("failed");
        assert!(failed.urgent && failed.body.contains(LOG));
        assert!(notice("started", &[]).is_some());
        assert!(notice("whatever", &[]).is_none());
    }
}
