//! Розбір виводу скрипта «на льоту»: етапи (рядки «==> …»), попередження й
//! помилки з порадами «що робити», прогрес компіляції ninja ([123/4567]),
//! прогрес завантаження curl, запитання, на які чекає скрипт (y/N, пароль
//! sudo), скасування і шлях до звіту. Тим самим розбором TUI читає й старі звіти.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    Running,
    Ok,
    Warn,
    Failed,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub title: String,
    pub started: Instant,
    pub ended: Option<Instant>,
    pub state: StepState,
}

impl Step {
    pub fn elapsed(&self, now: Instant) -> Duration {
        self.ended.unwrap_or(now).saturating_duration_since(self.started)
    }
}

/// Префікс рядка виводу скрипта. Скрипт друкує їх мовою інтерфейсу, а старі звіти — українською,
/// тож розуміємо обидві.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Step,
    Ok,
    Warning,
    Error,
    Hint,
    Fix,
    Cancelled,
    Report,
    Summary,
    Result,
}

const TAGS: &[(&str, Tag)] = &[
    ("==> ", Tag::Step),
    ("[OK] ", Tag::Ok),
    ("[УВАГА] ", Tag::Warning),
    ("[WARNING] ", Tag::Warning),
    ("[ПОМИЛКА] ", Tag::Error),
    ("[ERROR] ", Tag::Error),
    ("[ПОРАДА] ", Tag::Hint),
    ("[HINT] ", Tag::Hint),
    ("[ВИПРАВИТИ] ", Tag::Fix),
    ("[FIX] ", Tag::Fix),
    ("[СКАСОВАНО] ", Tag::Cancelled),
    ("[CANCELLED] ", Tag::Cancelled),
    ("Звіт: ", Tag::Report),
    ("Report: ", Tag::Report),
    ("Підсумок: ", Tag::Summary),
    ("Summary: ", Tag::Summary),
    ("Результат: ", Tag::Result),
    ("Result: ", Tag::Result),
];

/// Префікс рядка й решта тексту після нього.
pub fn tag(line: &str) -> Option<(Tag, &str)> {
    TAGS.iter().find_map(|(prefix, t)| line.strip_prefix(prefix).map(|rest| (*t, rest)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prompt {
    /// [y/N] або [Y/n].
    Confirm,
    /// Пароль sudo/doas.
    Password,
    /// Інший незавершений рядок, після якого вивід надовго затих.
    Input,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Warning,
    Error,
}

/// Що скрипт уміє виправити сам: «[ВИПРАВИТИ] команда: опис».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    /// Команда скрипта (deps, clean, check); TUI запускає лише дозволені.
    pub command: String,
    pub label: String,
}

/// Попередження чи помилка разом із порадами ([ПОРАДА]) і виправленням
/// ([ВИПРАВИТИ]), що йшли після неї.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub level: Level,
    pub text: String,
    pub hints: Vec<String>,
    pub fix: Option<Fix>,
}

#[derive(Debug, Clone, Copy)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    first_done: u64,
    first_at: Instant,
    last_at: Instant,
}

impl Progress {
    pub fn ratio(&self) -> f64 {
        if self.total == 0 { 0.0 } else { (self.done as f64 / self.total as f64).clamp(0.0, 1.0) }
    }

    /// Оцінка часу до завершення за середньою швидкістю.
    pub fn eta(&self) -> Option<Duration> {
        let elapsed = self.last_at.saturating_duration_since(self.first_at).as_secs_f64();
        let done = self.done.saturating_sub(self.first_done);
        if done < 5 || elapsed < 10.0 || self.done >= self.total {
            return None;
        }
        let rate = done as f64 / elapsed;
        Some(Duration::from_secs_f64((self.total - self.done) as f64 / rate))
    }
}

#[derive(Debug)]
pub struct Analyzer {
    pending: Vec<u8>,
    pub steps: Vec<Step>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub problems: Vec<Problem>,
    /// Поради, перед якими не було ні попередження, ні помилки.
    pub notes: Vec<String>,
    /// Користувач відмовився ([СКАСОВАНО]) — нічого не змінено.
    pub cancelled: Option<String>,
    pub compile: Option<Progress>,
    pub download: Option<f64>,
    pub report: Option<String>,
    /// «Підсумок: …» — що змінилося (напр., «OBS 33.0.0 → 33.0.1»).
    pub summary: Option<String>,
    pub last_line: String,
    partial: String,
    partial_at: Instant,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            steps: Vec::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
            problems: Vec::new(),
            notes: Vec::new(),
            cancelled: None,
            compile: None,
            download: None,
            report: None,
            summary: None,
            last_line: String::new(),
            partial: String::new(),
            partial_at: Instant::now(),
        }
    }

    pub fn feed(&mut self, bytes: &[u8], now: Instant) {
        self.pending.extend_from_slice(bytes);
        while let Some(pos) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=pos).collect();
            let text = visible_text(&String::from_utf8_lossy(&line[..line.len() - 1]));
            self.line(&text, now);
        }
        let partial = visible_text(&String::from_utf8_lossy(&self.pending));
        if partial != self.partial {
            self.partial = partial;
            self.partial_at = now;
        }
        if let Some(p) = curl_percent(&self.partial) {
            self.download = Some(p);
        }
    }

    fn current(&mut self) -> Option<&mut Step> {
        self.steps.last_mut()
    }

    fn line(&mut self, s: &str, now: Instant) {
        let s = s.trim_end();
        if s.is_empty() {
            return;
        }
        let tagged = tag(s);
        if let Some((Tag::Step, title)) = tagged {
            self.close_step(now, None);
            self.compile = None;
            self.download = None;
            self.steps.push(Step {
                title: title.trim().to_string(),
                started: now,
                ended: None,
                state: StepState::Running,
            });
        } else if let Some((Tag::Warning, msg)) = tagged {
            self.warnings.push(msg.to_string());
            self.problems.push(Problem { level: Level::Warning, text: msg.to_string(), hints: Vec::new(), fix: None });
            if let Some(step) = self.current()
                && step.state == StepState::Running
            {
                step.state = StepState::Warn;
            }
        } else if let Some((Tag::Error, msg)) = tagged {
            self.errors.push(msg.to_string());
            self.problems.push(Problem { level: Level::Error, text: msg.to_string(), hints: Vec::new(), fix: None });
            if let Some(step) = self.current() {
                step.state = StepState::Failed;
            }
        } else if let Some((Tag::Hint, msg)) = tagged {
            match self.problems.last_mut() {
                Some(p) => p.hints.push(msg.to_string()),
                None => self.notes.push(msg.to_string()),
            }
        } else if let Some((Tag::Fix, msg)) = tagged {
            if let (Some(p), Some((command, label))) = (self.problems.last_mut(), msg.split_once(": ")) {
                p.fix = Some(Fix { command: command.trim().to_string(), label: label.trim().to_string() });
            }
        } else if let Some((Tag::Cancelled, msg)) = tagged {
            self.cancelled = Some(msg.to_string());
        } else if let Some((Tag::Report, path)) = tagged {
            self.report = Some(path.trim().to_string());
        } else if let Some((Tag::Summary, text)) = tagged {
            self.summary = Some(text.trim().to_string());
        } else if let Some((done, total)) = ninja_progress(s) {
            let reset = match self.compile {
                Some(p) => p.total != total || done < p.done,
                None => true,
            };
            if reset {
                self.compile = Some(Progress { done, total, first_done: done, first_at: now, last_at: now });
            } else if let Some(p) = self.compile.as_mut() {
                p.done = done;
                p.last_at = now;
            }
        } else if curl_percent(s).is_some() {
            // Завантаження завершилося (curl дописав рядок до кінця).
            self.download = None;
        }
        if !s.starts_with('[') || ninja_progress(s).is_none() {
            self.last_line = s.to_string();
        }
    }

    /// Закриває поточний етап; `failed` — примусово позначити як невдалий.
    fn close_step(&mut self, now: Instant, failed: Option<bool>) {
        if let Some(step) = self.steps.last_mut()
            && step.ended.is_none()
        {
            step.ended = Some(now);
            match (failed, step.state) {
                (Some(true), _) => step.state = StepState::Failed,
                (_, StepState::Running) => step.state = StepState::Ok,
                _ => {}
            }
        }
    }

    /// Скрипт завершився.
    pub fn finish(&mut self, success: bool, now: Instant) {
        if !self.pending.is_empty() {
            let rest = std::mem::take(&mut self.pending);
            let text = visible_text(&String::from_utf8_lossy(&rest));
            self.line(&text, now);
        }
        self.partial.clear();
        self.download = None;
        let already_failed = self.steps.iter().any(|s| s.state == StepState::Failed);
        if self.cancelled.is_some() {
            // Відмова користувача — не помилка етапу.
            if let Some(step) = self.steps.last_mut()
                && step.ended.is_none()
            {
                step.state = StepState::Warn;
            }
            self.close_step(now, None);
        } else {
            self.close_step(now, (!success && !already_failed).then_some(true));
        }
    }

    /// Остання помилка — зазвичай саме вона пояснює, чому все зупинилося.
    pub fn last_error(&self) -> Option<&Problem> {
        self.problems.iter().rev().find(|p| p.level == Level::Error)
    }

    /// Найсвіжіше зауваження, до якого скрипт дав пораду чи виправлення, або просто останнє.
    pub fn last_notice(&self) -> Option<&Problem> {
        self.problems.iter().rev().find(|p| p.fix.is_some() || !p.hints.is_empty()).or_else(|| self.problems.last())
    }

    /// На яке запитання, схоже, чекає скрипт.
    pub fn prompt(&self, now: Instant) -> Option<Prompt> {
        let tail = self.partial.trim_end();
        if tail.is_empty() {
            return None;
        }
        let lower = tail.to_lowercase();
        if lower.contains("[y/n]") {
            Some(Prompt::Confirm)
        } else if lower.contains("password") || lower.contains("пароль") {
            Some(Prompt::Password)
        } else if (tail.ends_with(':') || tail.ends_with('?'))
            && now.saturating_duration_since(self.partial_at) > Duration::from_secs(2)
        {
            Some(Prompt::Input)
        } else {
            None
        }
    }
}

/// Текст рядка так, як його побачить людина: без ANSI-послідовностей і з
/// урахуванням повернення каретки (лишається текст після останнього \r).
pub fn visible_text(raw: &str) -> String {
    let raw = raw.strip_suffix('\r').unwrap_or(raw);
    let raw = raw.rsplit('\r').next().unwrap_or(raw);
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                Some('[') => {
                    // CSI: параметри, завершуються символом з діапазону @..~
                    for n in chars.by_ref() {
                        if ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC: до BEL або ESC \
                    while let Some(n) = chars.next() {
                        if n == '\x07' {
                            break;
                        }
                        if n == '\x1b' {
                            chars.next();
                            break;
                        }
                    }
                }
                Some('(' | ')' | '*' | '+') => {
                    chars.next();
                }
                _ => {}
            },
            '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// «[123/4567] Building …» → (123, 4567)
pub fn ninja_progress(s: &str) -> Option<(u64, u64)> {
    let rest = s.strip_prefix('[')?;
    let (inside, _) = rest.split_once(']')?;
    let (done, total) = inside.split_once('/')?;
    let done: u64 = done.trim().parse().ok()?;
    let total: u64 = total.trim().parse().ok()?;
    (total > 0 && done <= total).then_some((done, total))
}

/// Індикатор curl --progress-bar: «#####      45.2%» (у деяких локалях — «45,2%»).
pub fn curl_percent(s: &str) -> Option<f64> {
    let t = s.trim_end();
    let num = t.strip_suffix('%')?;
    if !t.trim_start().starts_with('#') && !t.trim_start().starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let start = num.rfind(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).map_or(0, |i| i + 1);
    let value: f64 = num[start..].replace(',', ".").parse().ok()?;
    (0.0..=100.0).contains(&value).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(a: &mut Analyzer, chunks: &[&str]) {
        let now = Instant::now();
        for c in chunks {
            a.feed(c.as_bytes(), now);
        }
    }

    #[test]
    fn strips_ansi_and_carriage_returns() {
        assert_eq!(visible_text("\x1b[34m\x1b[1m==>\x1b[0m Система\r"), "==> Система");
        assert_eq!(visible_text("###   10.0%\r#####  50.0%"), "#####  50.0%");
        assert_eq!(visible_text("\x1b]0;title\x07text"), "text");
    }

    #[test]
    fn summary_line() {
        let mut a = Analyzer::new();
        feed_all(&mut a, &["==> Що нового\n", "Підсумок: OBS 33.0.0 → 33.0.1; плагін waveform: додано\r\n"]);
        assert_eq!(a.summary.as_deref(), Some("OBS 33.0.0 → 33.0.1; плагін waveform: додано"));
        assert!(a.problems.is_empty());
    }

    #[test]
    fn tracks_steps_warnings_errors() {
        let mut a = Analyzer::new();
        feed_all(
            &mut a,
            &[
                "\x1b[34m\x1b[1m==>\x1b[0m Система\r\n  ОС: Artix\r\n",
                "\x1b[34m\x1b[1m==>\x1b[0m Паке",
                "ти для збірки\r\n\x1b[33m[УВАГА]\x1b[0m щось не так\r\n",
                "\x1b[34m\x1b[1m==>\x1b[0m CEF\r\n\x1b[31m[ПОМИЛКА]\x1b[0m зламано\r\n",
                "\r\nЗвіт: /home/u/.cache/x/reports/1-install.txt\r\n",
            ],
        );
        a.finish(false, Instant::now());
        let states: Vec<_> = a.steps.iter().map(|s| (s.title.as_str(), s.state)).collect();
        assert_eq!(
            states,
            [("Система", StepState::Ok), ("Пакети для збірки", StepState::Warn), ("CEF", StepState::Failed)]
        );
        assert_eq!(a.warnings, ["щось не так"]);
        assert_eq!(a.errors, ["зламано"]);
        assert_eq!(a.report.as_deref(), Some("/home/u/.cache/x/reports/1-install.txt"));
    }

    #[test]
    fn hints_attach_to_the_preceding_problem() {
        let mut a = Analyzer::new();
        feed_all(
            &mut a,
            &[
                "[ПОРАДА] ще до проблем\n",
                "==> Пакети\n\x1b[33m[УВАГА]\x1b[0m Ще не встановлено: swig\n",
                "\x1b[36m[ПОРАДА]\x1b[0m Їх поставить встановлення.\n",
                "==> CMake\n[ПОМИЛКА] CMake не зміг сконфігурувати OBS.\n[ПОРАДА] Встанови websocketpp.\n",
                "[ПОРАДА] І повтори.\n",
            ],
        );
        a.finish(false, Instant::now());
        assert_eq!(a.notes, ["ще до проблем"]);
        assert_eq!(a.problems.len(), 2);
        assert_eq!(a.problems[0].hints, ["Їх поставить встановлення."]);
        let e = a.last_error().unwrap();
        assert_eq!(e.text, "CMake не зміг сконфігурувати OBS.");
        assert_eq!(e.hints, ["Встанови websocketpp.", "І повтори."]);
        assert_eq!(a.last_notice().unwrap().text, e.text);
        assert_eq!(a.warnings.len(), 1);
        assert_eq!(a.errors.len(), 1);
    }

    #[test]
    fn fixes_attach_to_the_problem() {
        let mut a = Analyzer::new();
        feed_all(
            &mut a,
            &[
                "\x1b[33m[УВАГА]\x1b[0m Ще не встановлено: swig asio\n",
                "\x1b[32m[ВИПРАВИТИ]\x1b[0m deps: Встановити пакети: swig asio\n",
                "[ВИПРАВИТИ] без двокрапки\n",
            ],
        );
        let fix = a.problems[0].fix.as_ref().unwrap();
        assert_eq!(fix.command, "deps");
        assert_eq!(fix.label, "Встановити пакети: swig asio");
        assert_eq!(a.last_notice().unwrap().text, "Ще не встановлено: swig asio");
    }

    #[test]
    fn detects_cancel() {
        let mut a = Analyzer::new();
        feed_all(&mut a, &["Встановити ці пакети? [y/N]: n\r\n\x1b[33m[СКАСОВАНО]\x1b[0m Нічого не змінено.\r\n"]);
        a.finish(false, Instant::now());
        assert_eq!(a.cancelled.as_deref(), Some("Нічого не змінено."));
        assert!(a.errors.is_empty());
    }

    #[test]
    fn failure_without_error_line_marks_last_step() {
        let mut a = Analyzer::new();
        feed_all(&mut a, &["==> Компіляція OBS\n[1/10] cc a.c\n"]);
        a.finish(false, Instant::now());
        assert_eq!(a.steps[0].state, StepState::Failed);
    }

    #[test]
    fn ninja_progress_and_reset_between_builds() {
        let mut a = Analyzer::new();
        feed_all(&mut a, &["==> Компіляція OBS\n[1/100] a\n[50/100] b\n"]);
        let p = a.compile.unwrap();
        assert_eq!((p.done, p.total), (50, 100));
        feed_all(&mut a, &["==> Плагін obs-multi-rtmp\n[3/12] c\n"]);
        let p = a.compile.unwrap();
        assert_eq!((p.done, p.total), (3, 12));
        assert_eq!(ninja_progress("[ПОМИЛКА] x"), None);
        assert_eq!(ninja_progress("[OK] x"), None);
    }

    #[test]
    fn detects_prompts() {
        let now = Instant::now();
        let mut a = Analyzer::new();
        a.feed("Встановити ці пакети? [y/N]: ".as_bytes(), now);
        assert_eq!(a.prompt(now), Some(Prompt::Confirm));
        a.feed(b"y\r\n", now);
        assert_eq!(a.prompt(now), None);
        a.feed(b"[sudo] password for user: ", now);
        assert_eq!(a.prompt(now), Some(Prompt::Password));
        a.feed(b"\r\n:: Proceed with installation? [Y/n] ", now);
        assert_eq!(a.prompt(now), Some(Prompt::Confirm));
        a.feed(b"\r\nEnter a number (default=1):", now);
        assert_eq!(a.prompt(now), None);
        assert_eq!(a.prompt(now + Duration::from_secs(3)), Some(Prompt::Input));
    }

    #[test]
    fn curl_progress() {
        let mut a = Analyzer::new();
        let now = Instant::now();
        a.feed(b"##########                  25.5%", now);
        assert_eq!(a.download, Some(25.5));
        a.feed(b"\r############################ 100.0%\r\n", now);
        assert_eq!(a.download, None);
        assert_eq!(curl_percent("100.0%"), Some(100.0));
        assert_eq!(curl_percent("########     28,5%"), Some(28.5));
        assert_eq!(curl_percent("звантажено 5%"), None);
    }
}
