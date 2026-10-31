//! Вивід команд без меню (`--notifier`, `--check-updates`) у тому ж форматі, що й
//! у скрипта збирання: `==>`, `[OK]`, `[УВАГА]`, `[ПОМИЛКА]`, `[ПОРАДА]` (англійською —
//! `[WARNING]`, `[ERROR]`, `[HINT]`). TUI
//! запускає ці команди у вбудованому терміналі й розбирає вивід тим самим
//! аналізатором, тож картка результату показує поради й тут.

use std::io::{self, IsTerminal, Write};

fn line(color: &str, prefix: &str, msg: &str) {
    let mut out = io::stdout().lock();
    let _ = if io::stdout().is_terminal() {
        writeln!(out, "{color}{prefix}\x1b[0m {msg}")
    } else {
        writeln!(out, "{prefix} {msg}")
    };
}

pub fn say(msg: &str) {
    line("\x1b[34m\x1b[1m", "==>", msg);
}

pub fn ok(msg: &str) {
    line("\x1b[32m", "[OK]", msg);
}

pub fn warn(msg: &str) {
    line("\x1b[33m", tr!("[УВАГА]", "[WARNING]"), msg);
}

pub fn fail(msg: &str) {
    line("\x1b[31m", tr!("[ПОМИЛКА]", "[ERROR]"), msg);
}

pub fn hint(msg: &str) {
    line("\x1b[36m", tr!("[ПОРАДА]", "[HINT]"), msg);
}

/// Звичайний рядок з відступом (подробиці під попереднім повідомленням).
pub fn info(msg: &str) {
    println!("    {msg}");
}
