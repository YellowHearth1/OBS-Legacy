//! Дії з робочим столом: відкрити файл чи теку в типовій програмі, запустити програму окремо від
//! термінала, коротке сповіщення й буфер обміну.

use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::paths::find_in_path;

/// Відкриває файл або теку через xdg-open, не прив'язуючи програму до термінала.
pub fn open(path: &Path) -> Result<()> {
    if find_in_path("xdg-open").is_none() {
        bail!(tr!("немає xdg-open (пакет xdg-utils)", "xdg-open is missing (package xdg-utils)"));
    }
    spawn_detached(Command::new("xdg-open").arg(path))
        .context(tr!("не вдалося запустити xdg-open", "couldn't start xdg-open"))
}

/// Запускає програму окремо від термінала й сесії TUI (`setsid`), без вводу й виводу: вона живе й
/// після виходу з OBS-Legacy. На її завершення чекає окремий потік (`reap`).
pub fn spawn_detached(cmd: &mut Command) -> io::Result<()> {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // SAFETY: між fork і exec викликається лише setsid — безпечна для такого місця функція.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    reap(cmd.spawn()?);
    Ok(())
}

/// Коротке сповіщення на робочому столі (`notify-send`); немає його — нічого не робить.
pub fn notify(title: &str, body: &str) {
    if let Ok(child) = Command::new("notify-send")
        .args(["-a", "OBS-Legacy", title, body])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        reap(child);
    }
}

/// Дочекатися фонової програми в окремому потоці: інакше, завершившись, вона лишається
/// «зомбі», доки працює TUI.
pub fn reap(mut child: Child) {
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

/// Куди потрапив текст після `copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Copied {
    /// Програма буфера обміну (wl-copy, xclip чи xsel) підтвердила копіювання.
    Clipboard,
    /// Спрацював лише запит до термінала (OSC 52): чи підтримує його термінал, дізнатися не можна.
    TerminalOnly,
}

/// Копіює текст у буфер обміну: wl-copy (Wayland), xclip чи xsel (X11).
/// Додатково надсилає OSC 52 — так буфер заповнює сам термінал (Kitty,
/// WezTerm, foot, Alacritty…), навіть якщо цих програм немає.
pub fn copy(text: &str) -> Copied {
    osc52(text);
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let x11 = std::env::var_os("DISPLAY").is_some();
    let candidates: [(&str, &[&str], bool); 3] = [
        ("wl-copy", &[], wayland),
        ("xclip", &["-selection", "clipboard"], x11),
        ("xsel", &["--clipboard", "--input"], x11),
    ];
    for (program, args, usable) in candidates {
        if !usable || find_in_path(program).is_none() {
            continue;
        }
        let Ok(mut child) =
            Command::new(program).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if child.wait().is_ok_and(|s| s.success()) {
            return Copied::Clipboard;
        }
    }
    Copied::TerminalOnly
}

fn osc52(text: &str) {
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let _ = out.flush();
}

fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64("звіт".as_bytes()), "0LfQstGW0YI=");
    }
}
