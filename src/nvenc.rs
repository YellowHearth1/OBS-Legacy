//! Запит до драйвера: яку найвищу версію NVENC API він підтримує — так само,
//! як це робить OBS під час запуску. Виконується в окремому процесі
//! (`obs-legacy --probe-nvenc`), щоб збій у бібліотеці драйвера не зачепив TUI.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

type GetMaxSupportedVersion = unsafe extern "C" fn(*mut u32) -> i32;

/// Точка входу `--probe-nvenc`: друкує «13.0» і повертає код завершення.
pub fn probe_main() -> i32 {
    // SAFETY: dlopen/dlsym з коректними C-рядками; сигнатура функції
    // NvEncodeAPIGetMaxSupportedVersion задокументована в nvEncodeAPI.h.
    unsafe {
        let lib = libc::dlopen(c"libnvidia-encode.so.1".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if lib.is_null() {
            eprintln!("libnvidia-encode.so.1 не знайдено");
            return 2;
        }
        let sym = libc::dlsym(lib, c"NvEncodeAPIGetMaxSupportedVersion".as_ptr());
        if sym.is_null() {
            eprintln!("NvEncodeAPIGetMaxSupportedVersion не знайдено");
            return 3;
        }
        let get_version = std::mem::transmute::<*mut libc::c_void, GetMaxSupportedVersion>(sym);
        let mut version: u32 = 0;
        if get_version(&mut version) != 0 {
            eprintln!("драйвер не повідомив версію NVENC API");
            return 4;
        }
        println!("{}.{}", version >> 4, version & 0xF);
    }
    0
}

/// Запускає перевірку в окремому процесі з тайм-аутом.
pub fn query(exe: &Path) -> Option<String> {
    let mut child = Command::new(exe)
        .arg("--probe-nvenc")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    is_api_version(&text).then_some(text)
}

fn is_api_version(s: &str) -> bool {
    let mut parts = s.split('.');
    let ok = |p: Option<&str>| p.is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
}

/// "13.0" → 13*16+0, для порівняння версій API.
pub fn api_to_int(s: &str) -> Option<u32> {
    let (major, minor) = s.split_once('.')?;
    Some(major.parse::<u32>().ok()? * 16 + minor.parse::<u32>().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_versions() {
        assert!(is_api_version("13.0"));
        assert!(!is_api_version("13"));
        assert!(!is_api_version("13.0.1"));
        assert!(api_to_int("13.0") < api_to_int("13.1"));
        assert_eq!(api_to_int("12.2"), Some(12 * 16 + 2));
    }
}
