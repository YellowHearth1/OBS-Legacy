//! Клавіатура: що робить клавіша на кожному екрані і як вона потрапляє в скрипт, коли той питає.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::menu::{Action, Row};
use super::{App, Screen, ToastKind};
use crate::settings;

/// Українська розкладка (ЙЦУКЕН) → латинська літера на тій самій клавіші.
const UA_KEYS: &str = "йцукенгшщзфівапролдячсмить";
const LATIN_KEYS: &str = "qwertyuiopasdfghjklzxcvbnm";

/// Літера на тій самій клавіші в латинській розкладці. Скорочення прив'язані до клавіш, а не до літер:
/// з українською розкладкою «q» (на ній «й») так само виходить, а «o» («щ») так само відкриває звіт.
fn latin(c: char) -> char {
    let lower = c.to_lowercase().next().unwrap_or(c);
    match UA_KEYS.chars().position(|u| u == lower).and_then(|i| LATIN_KEYS.chars().nth(i)) {
        Some(l) if c.is_uppercase() => l.to_ascii_uppercase(),
        Some(l) => l,
        None => c,
    }
}

/// Код клавіші для скорочень (не для введення тексту): літера — як у латинській розкладці.
fn shortcut(code: KeyCode) -> KeyCode {
    match code {
        KeyCode::Char(c) => KeyCode::Char(latin(c)),
        other => other,
    }
}

fn ctrl(k: &KeyEvent, c: char) -> bool {
    k.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(k.code, KeyCode::Char(x) if latin(x).eq_ignore_ascii_case(&c))
}

/// Скільки чекати на друге Ctrl+C: випадкове одне натискання завдання не перериває.
const CTRL_C_WINDOW: Duration = Duration::from_secs(3);

/// Гортання виводу — однакове, працює завдання чи вже завершилося. `scroll` — рядків від низу,
/// `height` — висота вікна виводу. true — клавішу оброблено.
fn scroll_output(scroll: &mut usize, height: u16, code: KeyCode) -> bool {
    let page = usize::from(height.max(2) - 1);
    match code {
        KeyCode::PageUp => *scroll += page,
        KeyCode::PageDown => *scroll = scroll.saturating_sub(page),
        KeyCode::Up => *scroll += 1,
        KeyCode::Down => *scroll = scroll.saturating_sub(1),
        // До початку історії: vt100 сам обмежує зсув її довжиною.
        KeyCode::Home => *scroll = usize::MAX / 2,
        KeyCode::End => *scroll = 0,
        _ => return false,
    }
    true
}

/// Клавіша → байти для псевдотермінала.
pub fn key_to_bytes(k: &KeyEvent) -> Option<Vec<u8>> {
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    let mut out = match k.code {
        KeyCode::Char(c) if k.modifiers.contains(KeyModifiers::CONTROL) => {
            // Ctrl+літера в українській розкладці — той самий керівний код, що й на латиниці (Ctrl+С = ^C).
            let b = latin(c).to_ascii_lowercase() as u32;
            match b {
                0x61..=0x7a => vec![(b - 0x60) as u8],
                0x40 | 0x20 => vec![0],
                0x5b => vec![0x1b],
                0x5c => vec![0x1c],
                0x5d => vec![0x1d],
                _ => return None,
            }
        }
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        _ => return None,
    };
    if alt {
        out.insert(0, 0x1b);
    }
    Some(out)
}

impl App {
    pub(super) fn handle_event(&mut self, ev: Event) {
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => self.on_key(k),
            Event::Mouse(m) => self.on_mouse(m),
            Event::Paste(text) => self.on_paste(&text),
            _ => {}
        }
    }

    fn on_paste(&mut self, text: &str) {
        if self.screen == Screen::Run {
            if let Some(run) = self.run.as_mut() {
                run.scroll = 0;
                run.job.write_input(text.replace('\n', "\r").as_bytes());
            }
        } else if let Some(ed) = self.editor.as_mut() {
            ed.insert_str(text);
        }
    }

    pub(super) fn on_key(&mut self, k: KeyEvent) {
        if self.dialog.is_some() {
            // Як і решта скорочень — за розташуванням клавіш: в українській розкладці на Y — «н», на N — «т».
            match shortcut(k.code) {
                KeyCode::Enter | KeyCode::Char('y' | 'Y') => self.confirm_dialog(),
                KeyCode::Char('r' | 'R') if self.dialog.as_ref().is_some_and(|d| d.alt.is_some()) => self.alt_dialog(),
                KeyCode::Esc | KeyCode::Char('n' | 'N' | 'q' | 'Q') => self.dialog = None,
                _ => {}
            }
            return;
        }
        if self.screen != Screen::Run && ctrl(&k, 'c') {
            self.quit = true;
            return;
        }
        match self.screen {
            Screen::Intro => match shortcut(k.code) {
                KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right => self.go_home(),
                KeyCode::Esc | KeyCode::Char('q') => self.quit = true,
                _ => {}
            },
            Screen::Home => self.home_key(k),
            Screen::Run => self.run_key(k),
            Screen::Plugins => self.plugins_key(k),
            Screen::Rollback => self.rollback_key(k),
            Screen::Settings => self.settings_key(k),
            Screen::Reports => self.reports_key(k),
            Screen::Report => self.report_key(k),
            Screen::Help => self.help_key(k),
        }
    }

    fn home_key(&mut self, k: KeyEvent) {
        let rows = self.rows();
        let n = rows.len();
        self.menu_sel = self.menu_sel.min(n - 1);
        let mut touched = true;
        match shortcut(k.code) {
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => self.menu_sel = (self.menu_sel + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => self.menu_sel = (self.menu_sel + 1) % n,
            KeyCode::Home | KeyCode::Char('g') => self.menu_sel = 0,
            KeyCode::End | KeyCode::Char('G') => self.menu_sel = n - 1,
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l' | ' ') => self.activate_row(rows[self.menu_sel]),
            KeyCode::Left | KeyCode::Char('h') if self.advanced_open => self.collapse_advanced(),
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < n && rows[i] != Row::More {
                    self.menu_sel = i;
                }
            }
            KeyCode::Char('o') => self.view_current_report(),
            KeyCode::Char('d') => self.details = !self.details,
            KeyCode::Char('?') | KeyCode::F(1) => self.activate(Action::Help),
            KeyCode::Char('r') | KeyCode::F(5) => {
                self.refresh();
                self.toast(ToastKind::Info, tr!("Оновлюю відомості…", "Refreshing…"));
                touched = false;
            }
            // Esc спершу згортає «Додатково», а вже потім закриває програму.
            KeyCode::Esc if self.advanced_open => self.collapse_advanced(),
            KeyCode::Esc | KeyCode::Char('q') => self.quit = true,
            _ => touched = false,
        }
        self.menu_touched |= touched;
    }

    fn run_key(&mut self, k: KeyEvent) {
        let Some(run) = self.run.as_mut() else {
            self.go_home();
            return;
        };
        if scroll_output(&mut run.scroll, run.output_area.height, k.code) {
            return;
        }
        if !run.job.running() {
            match shortcut(k.code) {
                KeyCode::Enter => self.run_primary_activate(),
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => self.go_home(),
                KeyCode::Char('o') => self.view_current_report(),
                KeyCode::Char('c') => self.copy_report_path(),
                KeyCode::Char('e') => self.open_report_externally(false),
                _ => {}
            }
        } else if ctrl(&k, 'c') {
            self.interrupt_job();
        } else {
            self.forward_key(k);
        }
    }

    /// Ctrl+C: перше натискання лише попереджає, друге (протягом `CTRL_C_WINDOW`) надсилає Ctrl+C
    /// скрипту, а наступне після цього — зупиняє завдання примусово.
    fn interrupt_job(&mut self) {
        let Some(run) = self.run.as_mut() else { return };
        let now = Instant::now();
        let armed = run.ctrl_c_at.is_some_and(|t| now.duration_since(t) < CTRL_C_WINDOW);
        let message = if armed && run.interrupted {
            run.job.kill();
            run.ctrl_c_at = None;
            tr!("Завдання примусово зупинено.", "The task was killed.")
        } else if armed {
            run.job.interrupt();
            run.interrupted = true;
            run.ctrl_c_at = Some(now);
            tr!(
                "Надіслано Ctrl+C. Якщо скрипт не зупиниться — ще двічі Ctrl+C.",
                "Ctrl+C sent. If the script doesn't stop — press Ctrl+C twice more."
            )
        } else {
            run.ctrl_c_at = Some(now);
            tr!("Натисни Ctrl+C ще раз, щоб перервати завдання.", "Press Ctrl+C again to interrupt the task.")
        };
        self.toast(ToastKind::Warn, message);
    }

    /// Поки завдання працює, літери, Enter, Backspace, Tab і Ctrl+… ідуть у скрипт.
    fn forward_key(&mut self, k: KeyEvent) {
        // Скрипт питає лише y/N і пароль. Стрілки, Home/End, Esc йому не потрібні, а в рядку
        // введення лишилися б сміттям — і «y» на запитанні вже не зараховувалося б.
        match k.code {
            KeyCode::Esc => self.toast(
                ToastKind::Info,
                tr!("Завдання ще працює. Перервати — Ctrl+C.", "The task is still running. To interrupt — Ctrl+C."),
            ),
            KeyCode::Left | KeyCode::Right | KeyCode::Delete | KeyCode::Insert | KeyCode::BackTab => {}
            _ => {
                if let Some(bytes) = key_to_bytes(&k)
                    && let Some(run) = self.run.as_mut()
                {
                    run.scroll = 0;
                    run.job.write_input(&bytes);
                }
            }
        }
    }

    fn plugins_key(&mut self, k: KeyEvent) {
        let n = self.catalog.as_ref().map_or(0, Vec::len);
        match shortcut(k.code) {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('q') | KeyCode::Backspace => self.go_home(),
            KeyCode::Up | KeyCode::Char('k') if n > 0 => self.plugin_sel = (self.plugin_sel + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') if n > 0 => self.plugin_sel = (self.plugin_sel + 1) % n,
            KeyCode::Home => self.plugin_sel = 0,
            KeyCode::End if n > 0 => self.plugin_sel = n - 1,
            KeyCode::Char(' ' | 'x') | KeyCode::Enter if n > 0 => self.toggle_plugin(self.plugin_sel),
            KeyCode::Char('a') => self.set_plugins(|_| true),
            KeyCode::Char('n') => self.set_plugins(|_| false),
            KeyCode::Char('d') => self.set_plugins(|p| p.default),
            _ => {}
        }
    }

    fn rollback_key(&mut self, k: KeyEvent) {
        let n = self.builds.len();
        match shortcut(k.code) {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('q') | KeyCode::Backspace => self.go_home(),
            KeyCode::Up | KeyCode::Char('k') if n > 0 => self.build_sel = (self.build_sel + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') if n > 0 => self.build_sel = (self.build_sel + 1) % n,
            KeyCode::Enter if n > 0 => self.rollback_to(self.build_sel),
            _ => {}
        }
    }

    fn settings_key(&mut self, k: KeyEvent) {
        let n = settings::FIELDS.len();
        if let Some(ed) = self.editor.as_mut() {
            match k.code {
                KeyCode::Esc => {
                    self.editor = None;
                    self.settings_error = None;
                }
                KeyCode::Enter => {
                    let value = ed.text().trim().to_string();
                    self.commit_setting(self.settings_sel, &value);
                }
                _ => ed.key(k),
            }
            return;
        }
        match shortcut(k.code) {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('q') | KeyCode::Backspace => self.go_home(),
            KeyCode::Up | KeyCode::Char('k') => self.settings_sel = (self.settings_sel + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') => self.settings_sel = (self.settings_sel + 1) % n,
            KeyCode::Enter | KeyCode::Char(' ') => self.edit_setting(self.settings_sel),
            KeyCode::Delete | KeyCode::Char('r') => {
                let f = &settings::FIELDS[self.settings_sel];
                self.commit_setting(self.settings_sel, f.default);
            }
            _ => {}
        }
    }

    fn reports_key(&mut self, k: KeyEvent) {
        let n = self.reports.len();
        match shortcut(k.code) {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('q') | KeyCode::Backspace => self.go_home(),
            KeyCode::Up | KeyCode::Char('k') if n > 0 => self.reports_sel = (self.reports_sel + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') if n > 0 => self.reports_sel = (self.reports_sel + 1) % n,
            KeyCode::Home => self.reports_sel = 0,
            KeyCode::End if n > 0 => self.reports_sel = n - 1,
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('o') if n > 0 => {
                let path = self.reports[self.reports_sel].path.clone();
                self.open_report(path, Screen::Reports);
            }
            KeyCode::Char('c') => self.copy_report_path(),
            KeyCode::Char('e') => self.open_report_externally(false),
            KeyCode::Char('f') => self.open_report_externally(true),
            _ => {}
        }
    }

    fn report_key(&mut self, k: KeyEvent) {
        let Some(view) = self.report_view.as_mut() else {
            self.go_home();
            return;
        };
        let page = view.page.max(2) as isize - 1;
        match shortcut(k.code) {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('q') | KeyCode::Backspace => self.close_report(),
            KeyCode::Up | KeyCode::Char('k') => view.scroll_by(-1),
            KeyCode::Down | KeyCode::Char('j') => view.scroll_by(1),
            KeyCode::PageUp => view.scroll_by(-page),
            KeyCode::PageDown | KeyCode::Char(' ') => view.scroll_by(page),
            KeyCode::Home | KeyCode::Char('g') => view.scroll = 0,
            KeyCode::End | KeyCode::Char('G') => view.scroll = view.max_scroll(),
            KeyCode::Char(c @ ('n' | 'p')) => {
                if !view.jump_problem(c == 'n') {
                    self.toast(
                        ToastKind::Info,
                        tr!(
                            "Більше попереджень і помилок у цьому напрямку немає.",
                            "No more warnings or errors in this direction."
                        ),
                    );
                }
            }
            KeyCode::Char('c') => self.copy_report_path(),
            KeyCode::Char('e') => self.open_report_externally(false),
            KeyCode::Char('f') => self.open_report_externally(true),
            _ => {}
        }
    }

    fn help_key(&mut self, k: KeyEvent) {
        match shortcut(k.code) {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('q') | KeyCode::Backspace => self.go_home(),
            KeyCode::Up | KeyCode::Char('k') => self.help_scroll = self.help_scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.help_scroll = (self.help_scroll + 1).min(self.help_max_scroll),
            KeyCode::PageUp => self.help_scroll = self.help_scroll.saturating_sub(10),
            KeyCode::PageDown | KeyCode::Char(' ') => {
                self.help_scroll = (self.help_scroll + 10).min(self.help_max_scroll)
            }
            KeyCode::Home => self.help_scroll = 0,
            KeyCode::End => self.help_scroll = self.help_max_scroll,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn output_scrolls_by_lines_pages_and_ends() {
        let mut scroll = 0;
        assert!(scroll_output(&mut scroll, 11, KeyCode::PageUp));
        assert_eq!(scroll, 10);
        assert!(scroll_output(&mut scroll, 11, KeyCode::Down));
        assert_eq!(scroll, 9);
        assert!(scroll_output(&mut scroll, 11, KeyCode::PageDown));
        assert_eq!(scroll, 0);
        assert!(scroll_output(&mut scroll, 11, KeyCode::Down));
        assert_eq!(scroll, 0);
        assert!(scroll_output(&mut scroll, 11, KeyCode::Home));
        assert!(scroll > 1_000_000);
        assert!(scroll_output(&mut scroll, 11, KeyCode::End));
        assert_eq!(scroll, 0);
        // Літери й Enter — не гортання: вони йдуть у скрипт.
        assert!(!scroll_output(&mut scroll, 11, KeyCode::Char('y')));
        assert!(!scroll_output(&mut scroll, 11, KeyCode::Enter));
    }

    #[test]
    fn shortcuts_follow_key_positions_in_the_ukrainian_layout() {
        assert_eq!(UA_KEYS.chars().count(), LATIN_KEYS.len());
        for (ua, en) in [('й', 'q'), ('щ', 'o'), ('с', 'c'), ('у', 'e'), ('а', 'f'), ('в', 'd'), ('к', 'r')] {
            assert_eq!(latin(ua), en);
        }
        for (ua, en) in [('о', 'j'), ('л', 'k'), ('п', 'g'), ('П', 'G'), ('т', 'n'), ('з', 'p'), ('ф', 'a'), ('ч', 'x')]
        {
            assert_eq!(latin(ua), en);
        }
        assert_eq!(latin('Н'), 'Y');
        // Латиниця, цифри й знаки — як є.
        for c in ['q', 'G', '1', '?', ' '] {
            assert_eq!(latin(c), c);
        }
        assert_eq!(shortcut(KeyCode::Char('й')), KeyCode::Char('q'));
        assert_eq!(shortcut(KeyCode::Enter), KeyCode::Enter);
        assert!(ctrl(&key(KeyCode::Char('с'), KeyModifiers::CONTROL), 'c'));
        assert!(!ctrl(&key(KeyCode::Char('с'), KeyModifiers::NONE), 'c'));
        assert_eq!(key_to_bytes(&key(KeyCode::Char('в'), KeyModifiers::CONTROL)), Some(vec![4]));
    }

    #[test]
    fn keys_map_to_terminal_bytes() {
        assert_eq!(key_to_bytes(&key(KeyCode::Char('y'), KeyModifiers::NONE)), Some(b"y".to_vec()));
        assert_eq!(key_to_bytes(&key(KeyCode::Char('т'), KeyModifiers::NONE)), Some("т".as_bytes().to_vec()));
        assert_eq!(key_to_bytes(&key(KeyCode::Enter, KeyModifiers::NONE)), Some(b"\r".to_vec()));
        assert_eq!(key_to_bytes(&key(KeyCode::Char('c'), KeyModifiers::CONTROL)), Some(vec![3]));
        assert_eq!(key_to_bytes(&key(KeyCode::Char('d'), KeyModifiers::CONTROL)), Some(vec![4]));
        assert_eq!(key_to_bytes(&key(KeyCode::Up, KeyModifiers::NONE)), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_to_bytes(&key(KeyCode::Char('b'), KeyModifiers::ALT)), Some(b"\x1bb".to_vec()));
        assert_eq!(key_to_bytes(&key(KeyCode::F(5), KeyModifiers::NONE)), None);
    }
}
