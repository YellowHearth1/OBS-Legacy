//! Завдання у вбудованому терміналі: запуск скрипта й інших команд, стан запуску, головна кнопка
//! картки результату й запуск самого OBS.

use std::process::Command;
use std::time::Instant;

use ratatui::layout::Rect;

use super::dialogs::{Dialog, DialogAction};
use super::menu::{Action, fix_action, menu_item};
use super::{App, Screen, ToastKind};
use crate::analyzer::Prompt;
use crate::desktop;
use crate::launcher;
use crate::paths;
use crate::reports::{Outcome, ReportInfo};
use crate::runner::Job;
use crate::script::SelfSource;
use crate::settings;
use crate::system;
use crate::updates;

pub struct RunState {
    pub job: Job,
    pub action: Action,
    pub title: String,
    /// Зсув прокрутки (рядків від низу).
    pub scroll: usize,
    pub output_area: Rect,
    pub(super) ctrl_c_at: Option<Instant>,
    pub interrupted: bool,
    pub(super) last_prompt: Option<Prompt>,
    pub(super) finished_handled: bool,
    /// «Що далі» на картці успіху, якщо не типове для дії.
    pub next: Option<String>,
}

/// Команда «Оновити OBS-Legacy» для `bash`: `-c`, скрипт, `$0` і його аргументи. Тексти — окремими
/// аргументами ($2, $3…), а не всередині скрипта: так їх не зламає апостроф чи лапки в перекладі.
/// git і curl не питають логін і не висять на мертвому з'єднанні.
pub(super) fn self_update_args(src: &SelfSource) -> Vec<String> {
    match src {
        SelfSource::Git(dir) => vec![
            "-c".into(),
            "set -e; cd \"$1\"; echo \"==> git pull ($1)\"; GIT_TERMINAL_PROMPT=0 git pull --ff-only; \
             echo \"==> $2\"; exec ./install.sh --update"
                .into(),
            "obs-legacy-update".into(),
            dir.display().to_string(),
            tr!("Перевстановлення OBS-Legacy", "Reinstalling OBS-Legacy").into(),
        ],
        // --prebuilt: тимчасова тека не має вважатися теками з вихідними кодами.
        SelfSource::Release => vec![
            "-c".into(),
            "set -e; tmp=$(mktemp); trap 'rm -f \"$tmp\"' EXIT; echo \"==> $2: $1\"; \
             curl -fsSL --retry 3 --connect-timeout 20 --speed-limit 1 --speed-time 120 -o \"$tmp\" \"$1\" \
             || { echo \"$3\"; echo \"$4\"; exit 1; }; bash \"$tmp\" --prebuilt --update"
                .into(),
            "obs-legacy-update".into(),
            updates::INSTALLER_URL.into(),
            tr!("Завантажую install.sh з релізу", "Downloading install.sh from the release").into(),
            tr!(
                "[ПОМИЛКА] Не вдалося завантажити install.sh з релізу.",
                "[ERROR] Could not download install.sh from the release."
            )
            .into(),
            trf!(
                "[ПОРАДА] Перевір інтернет і чи відкривається {}, і спробуй ще раз.",
                "[HINT] Check the internet connection and that {} opens, then try again.",
                updates::RELEASES
            ),
        ],
    }
}

/// Розмір псевдотермінала до першого кадру: вікно виводу — це екран без смуг, рамок і відступів.
/// Точний розмір ui::run встановлює вже на першому кадрі (`Job::resize`).
fn initial_pty_size() -> (u16, u16) {
    let (cols, rows) = ratatui::crossterm::terminal::size().unwrap_or((launcher::COLS, launcher::ROWS));
    (rows.saturating_sub(9).max(5), cols.saturating_sub(4).max(20))
}

impl App {
    /// Чи йде зараз збирання: власне завдання або скрипт, запущений деінде (тримає блокування кешу).
    /// Блокування перевіряється лише тоді, коли останній звіт збирання без результату — щоб не
    /// відкривати файл на кожному кадрі.
    pub(super) fn build_running(&self) -> bool {
        self.run.as_ref().is_some_and(|r| r.job.running())
            || (self.last_build_report().is_some_and(|r| r.outcome == Outcome::Unknown) && system::build_running())
    }

    /// Останнє встановлення / оновлення / перезбирання обірвалося (звіт без «Результат:», а скрипт уже
    /// не працює) або завершилося помилкою.
    pub fn last_build_broken(&self) -> bool {
        let running = self.build_running();
        self.last_build_report()
            .is_some_and(|r| r.outcome == Outcome::Failed || (r.outcome == Outcome::Unknown && !running))
    }

    /// Найновіший звіт збирання OBS (встановлення, оновлення чи перезбирання).
    fn last_build_report(&self) -> Option<&ReportInfo> {
        self.reports.iter().find(|r| r.is_build())
    }

    /// Запустити команду скрипта. false — не запущено.
    pub(super) fn start_script(&mut self, action: Action, args: &[&str]) -> bool {
        let mut full = vec![self.script.display().to_string()];
        full.extend(args.iter().map(|s| s.to_string()));
        self.start_job(action, "bash", full)
    }

    /// Запустити завдання у вбудованому терміналі. false — не запущено.
    pub(super) fn start_job(&mut self, action: Action, program: &str, args: Vec<String>) -> bool {
        if self.run.as_ref().is_some_and(|r| r.job.running()) {
            self.screen = Screen::Run;
            self.toast(
                ToastKind::Warn,
                tr!("Спершу дочекайся завершення поточного завдання.", "Wait for the current task to finish first."),
            );
            return false;
        }
        let (rows, cols) = initial_pty_size();
        let env = [
            ("OBS_LEGACY_TUI", "1".to_string()),
            ("OBS_LEGACY_SETTINGS", settings::settings_path().display().to_string()),
            ("OBS_LEGACY_LANG", crate::i18n::lang().code().to_string()),
        ];
        match Job::spawn(program, &args, &paths::home(), &env, rows, cols) {
            Ok(job) => {
                let item = menu_item(action);
                self.run = Some(RunState {
                    job,
                    action,
                    title: item.label().to_string(),
                    scroll: 0,
                    output_area: Rect::default(),
                    ctrl_c_at: None,
                    interrupted: false,
                    last_prompt: None,
                    finished_handled: false,
                    next: None,
                });
                self.screen = Screen::Run;
                true
            }
            Err(e) => {
                self.toast(ToastKind::Err, trf!("Не вдалося запустити: {e:#}", "Couldn't start: {e:#}"));
                false
            }
        }
    }

    /// `anyway` — запустити навіть системний OBS, якщо власної збірки немає.
    pub(super) fn launch_obs(&mut self, anyway: bool) {
        let installed = self.sys.as_ref().is_some_and(|s| s.active_build().is_some());
        if !installed && !anyway {
            self.dialog = Some(Dialog {
                title: tr!("OBS-Legacy ще не встановлено", "OBS-Legacy isn't installed yet").into(),
                lines: vec![
                    tr!(
                        "Зараз команда obs запустить системний OBS — без NVENC на драйвері 580xx.",
                        "Right now the obs command starts the system OBS — without NVENC on the 580xx driver."
                    )
                    .into(),
                    tr!(
                        "Щоб отримати OBS з NVENC, спершу обери «Встановити / оновити OBS».",
                        "To get OBS with NVENC, choose “Install / update OBS” first."
                    )
                    .into(),
                ],
                confirm: tr!("Усе одно запустити", "Launch anyway"),
                danger: false,
                alt: None,
                action: DialogAction::LaunchSystemObs,
            });
            return;
        }
        let Some(obs) = paths::find_in_path("obs") else {
            self.toast(
                ToastKind::Err,
                tr!(
                    "Команду obs не знайдено — спершу «Встановити / оновити OBS».",
                    "The obs command isn't found — choose “Install / update OBS” first."
                ),
            );
            return;
        };
        match desktop::spawn_detached(&mut Command::new(&obs)) {
            Ok(()) => self.toast(ToastKind::Ok, trf!("OBS запускається ({})…", "OBS is starting ({})…", obs.display())),
            Err(e) => self.toast(ToastKind::Err, trf!("Не вдалося запустити OBS: {e}", "Couldn't start OBS: {e}")),
        }
    }

    /// Головна кнопка картки результату: виправлення, яке запропонував скрипт, або
    /// після встановлення пакетів — повторна перевірка.
    pub fn run_primary(&self) -> Option<Action> {
        let run = self.run.as_ref()?;
        let exit = run.job.exit?;
        let a = &run.job.analyzer;
        if run.interrupted || a.cancelled.is_some() {
            return None;
        }
        let problem = if exit.success { a.last_notice() } else { a.last_error().or_else(|| a.last_notice()) };
        let fix = problem.and_then(|p| p.fix.as_ref()).and_then(|f| fix_action(&f.command));
        // Пакети вже поставили (наприклад, вручну) — тоді краще перевірити ще раз.
        let fix = match fix {
            Some(Action::Deps) if self.missing_packages.as_ref().is_some_and(Vec::is_empty) => Some(Action::Check),
            other => other,
        };
        match fix {
            Some(action) if action != run.action || !exit.success => Some(action),
            _ => (exit.success && run.action == Action::Deps).then_some(Action::Check),
        }
    }

    /// Enter або клац на головній кнопці картки результату.
    pub(super) fn run_primary_activate(&mut self) {
        let primary = self.run_primary();
        self.go_home();
        if let Some(action) = primary {
            self.select_action(action);
            self.activate(action);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn self_update_command_takes_texts_as_arguments() {
        for src in [SelfSource::Git(PathBuf::from("/home/u/OBS-Legacy")), SelfSource::Release] {
            let args = self_update_args(&src);
            assert_eq!(args[0], "-c");
            let script = &args[1];
            // Перекладу (кирилиці) в самому скрипті немає — лише в аргументах.
            assert!(script.is_ascii(), "{script}");
            // Кожен $N, на який посилається скрипт, переданий окремим аргументом (args[2] — це $0).
            let used = (1..=9).filter(|n| script.contains(&format!("${n}"))).max().unwrap_or(0);
            assert_eq!(args.len(), 3 + used, "{args:?}");
        }
    }
}
