//! Звіти скрипта: `~/.cache/obs-studio-nvenc-artix/reports/<дата>-<команда>.txt`.
//! Список запусків, їхній результат і головна проблема (для головного екрана)
//! та текст для вбудованого переглядача. Лише читає.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::analyzer::{Analyzer, Problem, Tag, tag};
use crate::script;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Success,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ReportInfo {
    pub path: PathBuf,
    /// Мітка часу з назви файла: «20261028-174550».
    pub stamp: String,
    /// Команда скрипта: check, install, rollback, uninstall.
    pub command: String,
    pub outcome: Outcome,
    pub warnings: usize,
    /// Що показати людині: для невдалого запуску — остання помилка, для
    /// успішного — зауваження, до якого скрипт дав пораду.
    pub problem: Option<Problem>,
}

impl ReportInfo {
    pub fn label(&self) -> &'static str {
        command_label(&self.command)
    }
}

pub fn dir() -> PathBuf {
    script::cache_home().join("obs-studio-nvenc-artix").join("reports")
}

pub fn command_label(command: &str) -> &'static str {
    match command {
        "check" => "Перевірка системи",
        "install" => "Встановлення OBS",
        "update" => "Оновлення OBS",
        "rebuild" => "Перезбирання OBS",
        "package" => "Збирання пакетів",
        "rollback" => "Відкат",
        "uninstall" => "Видалення OBS-Legacy",
        "deps" => "Пакети для збірки",
        _ => "Інше",
    }
}

/// Сьогоднішня дата як «YYYYmmdd» за місцевим часом.
fn today() -> String {
    // SAFETY: time і localtime_r заповнюють передані структури.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return String::new();
        }
        format!("{:04}{:02}{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday)
    }
}

/// «20261028-174550» → «сьогодні 17:45» або «28.10.2026 17:45».
pub fn when(stamp: &str) -> String {
    when_relative(stamp, &today())
}

fn when_relative(stamp: &str, today: &str) -> String {
    let (date, time) = stamp.split_once('-').unwrap_or((stamp, ""));
    if date.len() != 8 || time.len() < 4 || !date.bytes().chain(time.bytes()).all(|b| b.is_ascii_digit()) {
        return stamp.to_string();
    }
    let hm = format!("{}:{}", &time[..2], &time[2..4]);
    if date == today {
        format!("сьогодні {hm}")
    } else {
        format!("{}.{}.{} {hm}", &date[6..8], &date[4..6], &date[..4])
    }
}

/// Останні `max` байтів файла: у великих звітах збирання важливий кінець.
fn read_tail(path: &Path, max: u64) -> Option<String> {
    let mut f = fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    if len > max {
        f.seek(SeekFrom::Start(len - max)).ok()?;
    }
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Результат і головна проблема за текстом звіту.
pub fn analyze(text: &str) -> (Outcome, usize, Option<Problem>) {
    let mut a = Analyzer::new();
    a.feed(text.as_bytes(), Instant::now());
    a.feed(b"\n", Instant::now());
    let result = text.lines().rev().find_map(|l| match tag(l.trim()) {
        Some((Tag::Result, r)) => Some(r),
        _ => None,
    });
    let outcome = match result {
        Some(r) if r.starts_with("успішно") || r.starts_with("success") => Outcome::Success,
        Some(r) if r.starts_with("скасовано") || r.starts_with("cancelled") => Outcome::Cancelled,
        Some(r) if r.starts_with("помилка") || r.starts_with("error") => Outcome::Failed,
        // Звіти старіших версій скрипта — без рядка «Результат».
        _ if a.cancelled.is_some() => Outcome::Cancelled,
        _ if ["ПЕРЕВІРКУ ПРОЙДЕНО", "ПЕРЕВІРКУ ЗАВЕРШЕНО", "ГОТОВО:"].iter().any(|m| text.contains(m)) => {
            Outcome::Success
        }
        _ if !a.errors.is_empty() => Outcome::Failed,
        _ => Outcome::Unknown,
    };
    let problem = match outcome {
        Outcome::Failed => a.last_error().or_else(|| a.last_notice()).cloned(),
        Outcome::Success | Outcome::Unknown => a.last_notice().filter(|p| !p.hints.is_empty()).cloned(),
        Outcome::Cancelled => None,
    };
    (outcome, a.warnings.len(), problem)
}

pub fn summarize(path: &Path) -> Option<ReportInfo> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".txt")?;
    // «20261028-174550-check» → мітка часу і команда.
    let (stamp, command) = stem.rsplit_once('-')?;
    if stamp.len() != 15 {
        return None;
    }
    let text = read_tail(path, 64 * 1024)?;
    let (outcome, warnings, problem) = analyze(&text);
    Some(ReportInfo {
        path: path.to_path_buf(),
        stamp: stamp.to_string(),
        command: command.to_string(),
        outcome,
        warnings,
        problem,
    })
}

/// Усі звіти, від найновішого.
pub fn list() -> Vec<ReportInfo> {
    list_in(&dir())
}

pub fn list_in(dir: &Path) -> Vec<ReportInfo> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "txt")).collect()
        })
        .unwrap_or_default();
    paths.sort();
    paths.reverse();
    // Сотні звітів за роки не потрібні — вистачить найсвіжіших.
    paths.into_iter().take(200).filter_map(|p| summarize(&p)).collect()
}

/// Повний текст звіту для переглядача (дуже великі — лише кінець).
pub fn load_lines(path: &Path) -> std::io::Result<Vec<String>> {
    let text = read_tail(path, 8 * 1024 * 1024)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "не вдалося прочитати файл"))?;
    Ok(text.lines().map(|l| l.replace('\t', "    ")).collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Step,
    Ok,
    Warn,
    Error,
    Hint,
    Cancel,
    Plain,
}

impl LineKind {
    /// Попередження, помилки й скасування — те, між чим стрибає переглядач.
    pub fn is_problem(self) -> bool {
        matches!(self, LineKind::Warn | LineKind::Error | LineKind::Cancel)
    }
}

pub fn classify(line: &str) -> LineKind {
    match tag(line) {
        Some((Tag::Step, _)) => LineKind::Step,
        Some((Tag::Ok, _)) => LineKind::Ok,
        Some((Tag::Warning, _)) => LineKind::Warn,
        Some((Tag::Error, _)) => LineKind::Error,
        Some((Tag::Hint, _)) => LineKind::Hint,
        Some((Tag::Cancelled, _)) => LineKind::Cancel,
        _ => LineKind::Plain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_dates() {
        assert_eq!(when_relative("20261028-174550", "20261028"), "сьогодні 17:45");
        assert_eq!(when_relative("20261027-090102", "20261028"), "27.10.2026 09:01");
        assert_eq!(when_relative("дивно", "20261028"), "дивно");
    }

    #[test]
    fn outcome_from_result_line_and_old_reports() {
        let (o, _, p) = analyze("==> Х\n[ПОМИЛКА] Тег не той.\n[ПОРАДА] Видали кеш.\n\nРезультат: помилка (код 1)\n");
        assert_eq!(o, Outcome::Failed);
        let p = p.unwrap();
        assert_eq!(p.text, "Тег не той.");
        assert_eq!(p.hints, ["Видали кеш."]);

        // Звіт старої версії скрипта: без «Результат», але з помилкою.
        let (o, _, _) = analyze("==> nv-codec-headers\n[ПОМИЛКА] Очікувалась версія 13.0.19.1\n");
        assert_eq!(o, Outcome::Failed);

        let (o, w, p) = analyze(
            "[УВАГА] Ще не встановлено: swig\n[ПОРАДА] Їх поставить встановлення.\n\
             ПЕРЕВІРКУ ЗАВЕРШЕНО: усе готово\nРезультат: успішно\n",
        );
        assert_eq!((o, w), (Outcome::Success, 1));
        assert_eq!(p.unwrap().hints, ["Їх поставить встановлення."]);

        let (o, _, p) = analyze("[СКАСОВАНО] Нічого не змінено.\nРезультат: скасовано\n");
        assert_eq!(o, Outcome::Cancelled);
        assert!(p.is_none());
    }

    #[test]
    fn lists_reports_newest_first() {
        let dir = std::env::temp_dir().join(format!("obs-legacy-reports-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("20261028-173417-check.txt"), "[ПОМИЛКА] стара\n").unwrap();
        fs::write(dir.join("20261028-174550-check.txt"), "ПЕРЕВІРКУ ПРОЙДЕНО\nРезультат: успішно\n").unwrap();
        fs::write(dir.join("нотатки.txt"), "не звіт").unwrap();
        let list = list_in(&dir);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].stamp, "20261028-174550");
        assert_eq!(list[0].command, "check");
        assert_eq!(list[0].outcome, Outcome::Success);
        assert_eq!(list[1].outcome, Outcome::Failed);
        assert_eq!(list[1].label(), "Перевірка системи");
    }

    #[test]
    fn classifies_lines() {
        assert_eq!(classify("==> Система"), LineKind::Step);
        assert_eq!(classify("[ПОРАДА] x"), LineKind::Hint);
        assert!(classify("[УВАГА] x").is_problem());
        assert!(!classify("  звичайний рядок").is_problem());
    }
}
