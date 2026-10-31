//! Якщо obs-legacy запустили клацанням (з меню програм чи файлового
//! менеджера) без термінала, він сам відкриває вікно термінала і
//! перезапускається в ньому — одразу з меню.

use std::env;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Result, bail};

const COLS: u16 = 120;
const ROWS: u16 = 36;
const TITLE: &str = "OBS-Legacy";

/// Командний рядок для запуску `program` у конкретному терміналі.
pub fn terminal_command(term: &str, program: &Path) -> Vec<OsString> {
    let p = program.as_os_str().to_owned();
    let geometry = format!("{COLS}x{ROWS}");
    let s = |v: &str| OsString::from(v);
    match term {
        "xdg-terminal-exec" => vec![s(term), p],
        "kitty" => vec![
            s(term),
            s("--title"),
            s(TITLE),
            s("-o"),
            s(&format!("initial_window_width={COLS}c")),
            s("-o"),
            s(&format!("initial_window_height={ROWS}c")),
            p,
        ],
        "alacritty" => vec![
            s(term),
            s("--title"),
            s(TITLE),
            s("-o"),
            s(&format!("window.dimensions.columns={COLS}")),
            s("-o"),
            s(&format!("window.dimensions.lines={ROWS}")),
            s("-e"),
            p,
        ],
        "wezterm" => vec![
            s(term),
            s("--config"),
            s(&format!("initial_cols={COLS}")),
            s("--config"),
            s(&format!("initial_rows={ROWS}")),
            s("start"),
            s("--"),
            p,
        ],
        "foot" => vec![s(term), s("--title"), s(TITLE), s(&format!("--window-size-chars={geometry}")), p],
        "ghostty" => vec![s(term), s(&format!("--title={TITLE}")), s("-e"), p],
        "gnome-terminal" | "mate-terminal" => {
            vec![s(term), s(&format!("--geometry={geometry}")), s("--title"), s(TITLE), s("--"), p]
        }
        "xfce4-terminal" => vec![s(term), s(&format!("--geometry={geometry}")), s("--title"), s(TITLE), s("-x"), p],
        "terminator" => vec![s(term), s("--title"), s(TITLE), s("-x"), p],
        "xterm" | "urxvt" | "rxvt" => vec![s(term), s("-geometry"), s(&geometry), s("-title"), s(TITLE), s("-e"), p],
        "st" => vec![s(term), s("-g"), s(&geometry), s("-t"), s(TITLE), s("-e"), p],
        // konsole, qterminal, lxterminal, tilix, kgx, ptyxis та інші
        _ => vec![s(term), s("-e"), p],
    }
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path).map(|d| d.join(name)).find(|p| {
        p.metadata()
            .map(|m| m.is_file() && std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o111 != 0)
            .unwrap_or(false)
    })
}

/// Порядок пошуку: $TERMINAL, xdg-terminal-exec, термінал поточного
/// робочого середовища, далі — поширені термінали.
pub fn candidates() -> Vec<String> {
    let mut list: Vec<String> = Vec::new();
    if let Ok(t) = env::var("TERMINAL") {
        let t = t.trim();
        if !t.is_empty() {
            list.push(t.to_string());
        }
    }
    list.push("xdg-terminal-exec".into());
    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_uppercase();
    let de_first: &[&str] = if desktop.contains("KDE") {
        &["konsole"]
    } else if desktop.contains("GNOME") || desktop.contains("UNITY") || desktop.contains("CINNAMON") {
        &["kgx", "ptyxis", "gnome-terminal"]
    } else if desktop.contains("XFCE") {
        &["xfce4-terminal"]
    } else if desktop.contains("MATE") {
        &["mate-terminal"]
    } else if desktop.contains("LXQT") {
        &["qterminal"]
    } else if desktop.contains("LXDE") {
        &["lxterminal"]
    } else {
        &[]
    };
    list.extend(de_first.iter().map(|s| s.to_string()));
    for t in [
        "kitty",
        "alacritty",
        "wezterm",
        "foot",
        "ghostty",
        "konsole",
        "gnome-terminal",
        "xfce4-terminal",
        "mate-terminal",
        "qterminal",
        "lxterminal",
        "terminator",
        "tilix",
        "urxvt",
        "st",
        "xterm",
    ] {
        if !list.iter().any(|x| x == t) {
            list.push(t.into());
        }
    }
    list
}

fn notify(message: &str) {
    let _ = Command::new("notify-send")
        .args(["-a", TITLE, TITLE, message])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Змінна, за якою дочірній екземпляр знає, що його вже перезапустили:
/// якщо «термінал» не дав йому TTY, він не почне відкривати вікна по колу.
pub const NO_RELAUNCH_ENV: &str = "OBS_LEGACY_NO_RELAUNCH";

pub fn relaunch_in_terminal() -> Result<()> {
    let exe = env::current_exe()?;
    for term in candidates() {
        // $TERMINAL може містити параметри: "alacritty --class foo".
        let mut words = term.split_whitespace();
        let Some(bin) = words.next() else { continue };
        if find_in_path(bin).is_none() {
            continue;
        }
        let extra: Vec<&str> = words.collect();
        let name = Path::new(bin).file_name().and_then(|n| n.to_str()).unwrap_or(bin);
        let argv = terminal_command(name, &exe);
        let mut cmd = Command::new(bin);
        cmd.args(extra)
            .args(&argv[1..])
            .env(NO_RELAUNCH_ENV, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        if cmd.spawn().is_ok() {
            return Ok(());
        }
    }
    let msg = tr!(
        "Не знайдено термінала. Встанови, напр., kitty, alacritty або konsole, чи задай змінну TERMINAL.",
        "No terminal found. Install one, e.g. kitty, alacritty or konsole, or set the TERMINAL variable."
    );
    notify(msg);
    bail!(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_known_command_lines() {
        let p = Path::new("/home/u/.local/bin/obs-legacy");
        let s = |v: Vec<OsString>| v.into_iter().map(|x| x.into_string().unwrap()).collect::<Vec<_>>();
        assert_eq!(s(terminal_command("konsole", p)), ["konsole", "-e", "/home/u/.local/bin/obs-legacy"]);
        assert_eq!(s(terminal_command("xdg-terminal-exec", p)), ["xdg-terminal-exec", "/home/u/.local/bin/obs-legacy"]);
        let g = s(terminal_command("gnome-terminal", p));
        assert_eq!(g.last().unwrap(), "/home/u/.local/bin/obs-legacy");
        assert!(g.contains(&"--".to_string()));
        assert!(s(terminal_command("xterm", p)).contains(&"120x36".to_string()));
    }

    #[test]
    fn candidates_have_no_duplicates() {
        let c = candidates();
        let mut sorted = c.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), c.len());
    }
}
