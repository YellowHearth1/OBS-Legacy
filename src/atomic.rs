//! Надійний запис файлу: тимчасовий файл поруч, fsync, перейменування, fsync теки. Після збою
//! живлення чи падіння системи на диску лишається або старий вміст, або новий — ніколи порожній
//! чи обрізаний файл (без fsync так буває на btrfs і xfs, а на ext4 — з `noauto_da_alloc`).

use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

/// Записує `bytes` у `path` з правами `mode`; теки створює за потреби.
pub fn write(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, tr!("шлях без назви файлу", "path has no file name"))
    })?;
    fs::create_dir_all(dir)?;
    // Номер виклику — на випадок, якщо два потоки програми колись запишуть той самий файл одночасно.
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let tmp = dir.join(format!(
        ".{}.tmp{}-{}",
        name.to_string_lossy(),
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let result = write_and_rename(&tmp, path, dir, bytes, mode);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn write_and_rename(tmp: &Path, path: &Path, dir: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    let _ = fs::remove_file(tmp);
    let mut file = OpenOptions::new().write(true).create_new(true).mode(mode).open(tmp)?;
    file.write_all(bytes)?;
    // umask могла зрізати права під час створення — ставимо їх явно.
    file.set_permissions(Permissions::from_mode(mode))?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, path)?;
    // Саме перейменування — запис у теці; без fsync теки воно теж може загубитися.
    File::open(dir)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("obs-legacy-atomic-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn writes_creates_dirs_and_replaces() {
        let dir = temp_dir("replace");
        let path = dir.join("a/b/settings.env");
        write(&path, b"first\n", 0o644).unwrap();
        write(&path, b"second\n", 0o600).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second\n");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        // Тимчасових файлів не лишилося.
        let leftovers: Vec<_> = fs::read_dir(dir.join("a/b")).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(leftovers.len(), 1, "{leftovers:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn failure_leaves_the_old_file() {
        let dir = temp_dir("fail");
        let path = dir.join("state.env");
        write(&path, b"old\n", 0o644).unwrap();
        // Ціль — тека: перейменування файлу поверх неї не вдасться, старий вміст поруч цілий.
        let blocked = dir.join("blocked");
        fs::create_dir_all(blocked.join("inner")).unwrap();
        assert!(write(&blocked, b"new\n", 0o644).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "old\n");
        let names: Vec<_> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert!(names.iter().all(|n| !n.to_string_lossy().contains(".tmp")), "{names:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}
