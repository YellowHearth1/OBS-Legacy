//! Стан програми й головний цикл: що відкрито, що вже відомо про систему, що показує екран.
//! Решта — у підмодулях за темами: меню й дії, діалоги, завдання у вбудованому терміналі, фонові
//! відомості, клавіатура, миша, списки (плагіни, відкат, налаштування) і переглядач звітів.

mod dialogs;
mod editor;
mod input;
mod jobs;
mod lists;
mod menu;
mod mouse;
mod report_view;
mod tasks;

use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event;
use ratatui::layout::Rect;

use crate::advice::{self, Advice};
use crate::builds::BuildInfo;
use crate::catalog::PluginInfo;
use crate::desktop;
use crate::reports::ReportInfo;
use crate::script::{self, SelfSource};
use crate::service;
use crate::settings::Settings;
use crate::system::SysInfo;
use crate::theme::Theme;
use crate::ui;
use crate::updates;
use crate::versions::Updates;

pub use dialogs::Dialog;
pub use editor::Editor;
pub use jobs::RunState;
pub use menu::{Action, MenuItem, Row, fix_action, menu_item};
pub use mouse::Target;
pub use report_view::ReportView;
use tasks::Msg;

/// Як часто перемальовувати екран, поки щось рухається (вивід завдання, логотип на першому екрані).
const FRAME_BUSY: Duration = Duration::from_millis(60);
/// Як часто — коли нічого не відбувається: менше роботи для процесора.
const FRAME_IDLE: Duration = Duration::from_millis(200);
/// Скільки видно повідомлення внизу екрана.
const TOAST_FOR: Duration = Duration::from_secs(5);
/// Після якого часу завдання вважається довгим: тоді по завершенні ще й дзвінок і сповіщення.
const LONG_TASK: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Intro,
    Home,
    Run,
    Plugins,
    Rollback,
    Settings,
    Reports,
    Report,
    Help,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Ok,
    Warn,
    Err,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    until: Instant,
}

/// Чим закінчилося завдання.
struct Finished {
    success: bool,
    /// Людина відмовилася ([СКАСОВАНО]) або перервала (Ctrl+C).
    cancelled: bool,
    elapsed: Duration,
    action: Action,
}

pub struct App {
    pub theme: Theme,
    pub screen: Screen,
    pub exe: PathBuf,
    pub script: PathBuf,
    pub settings: Settings,
    pub menu_sel: usize,
    /// Чи розгорнуто розділ «Додатково».
    pub advanced_open: bool,
    /// Людина вже рухала вибір у меню — більше не підставляти пораду.
    menu_touched: bool,
    /// Панель «Твоя система» докладно (d).
    pub details: bool,
    pub sys: Option<SysInfo>,
    pub sys_loading: bool,
    pub updates: Option<Updates>,
    pub updates_loading: bool,
    updates_requested: bool,
    pub reports: Vec<ReportInfo>,
    pub reports_loaded: bool,
    /// Пакети для збірки, яких бракує (None — ще невідомо).
    pub missing_packages: Option<Vec<String>>,
    /// Обов'язкові пакети, яких немає в репозиторіях.
    pub missing_norepo: Vec<String>,
    /// Сповіщення про оновлення: None — ще невідомо, Some(None) — вимкнено.
    pub notifier: Option<Option<service::Backend>>,
    pub reports_sel: usize,
    pub report_view: Option<ReportView>,
    pub catalog: Option<Vec<PluginInfo>>,
    pub catalog_error: Option<String>,
    pub plugin_sel: usize,
    pub builds: Vec<BuildInfo>,
    pub build_sel: usize,
    pub settings_sel: usize,
    pub editor: Option<Editor>,
    pub settings_error: Option<String>,
    pub help_scroll: u16,
    pub help_max_scroll: u16,
    pub run: Option<RunState>,
    pub dialog: Option<Dialog>,
    pub toast: Option<Toast>,
    pub hits: Vec<(Rect, Target)>,
    pub tick: u64,
    pub self_source: Option<SelfSource>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    quit: bool,
}

fn bell() {
    let mut out = std::io::stdout();
    let _ = out.write_all(b"\x07");
    let _ = out.flush();
}

impl App {
    pub fn new(exe: PathBuf, script: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let settings = Settings::load();
        Self {
            theme: Theme::detect(settings.solid_background()),
            screen: Screen::Intro,
            exe,
            script,
            settings,
            menu_sel: 0,
            advanced_open: false,
            menu_touched: false,
            details: false,
            sys: None,
            sys_loading: false,
            updates: None,
            updates_loading: false,
            updates_requested: false,
            reports: Vec::new(),
            reports_loaded: false,
            missing_packages: None,
            missing_norepo: Vec::new(),
            notifier: None,
            reports_sel: 0,
            report_view: None,
            catalog: None,
            catalog_error: None,
            plugin_sel: 0,
            builds: Vec::new(),
            build_sel: 0,
            settings_sel: 0,
            editor: None,
            settings_error: None,
            help_scroll: 0,
            help_max_scroll: 0,
            run: None,
            dialog: None,
            toast: None,
            hits: Vec::new(),
            tick: 0,
            self_source: script::self_source(),
            tx,
            rx,
            quit: false,
        }
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        self.spawn_catalog();
        self.spawn_sys();
        self.spawn_reports();
        self.spawn_deps();
        while !self.quit {
            self.process_messages();
            self.update();
            terminal.draw(|f| ui::draw(f, &mut self))?;
            self.tick = self.tick.wrapping_add(1);
            let busy = self.screen == Screen::Intro || self.run.as_ref().is_some_and(|r| r.job.running());
            if event::poll(if busy { FRAME_BUSY } else { FRAME_IDLE })? {
                loop {
                    let ev = event::read()?;
                    self.handle_event(ev);
                    if self.quit || !event::poll(Duration::ZERO)? {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    /// Порада «Що далі» (коли відомості про систему вже зібрано).
    pub fn advice(&self) -> Option<Advice> {
        let sys = self.sys.as_ref()?;
        let plugins = self.settings.plugins();
        let facts = self.facts();
        Some(advice::advise(&advice::Input {
            sys,
            facts: facts.as_ref(),
            reports: &self.reports,
            selected_plugins: &plugins,
            missing_packages: self.missing_packages.as_deref(),
            self_update: self.self_source.is_some(),
            notifier: self.notifier.map(|n| n.is_some()),
            build_running: self.build_running(),
        }))
    }

    /// Нові версії OBS, плагінів і OBS-Legacy — так само, як їх бачать сповіщення.
    pub fn facts(&self) -> Option<updates::Facts> {
        let u = self.updates.as_ref()?;
        let active = self.sys.as_ref().and_then(SysInfo::active_build);
        let catalog = self.catalog.as_deref().unwrap_or_default();
        Some(updates::facts_from(active, u, &self.settings, catalog, self.self_source.is_some()))
    }

    /// Що змінилося між кадрами: сповіщення внизу згасає, завдання видає новий вивід чи завершується.
    fn update(&mut self) {
        if self.toast.as_ref().is_some_and(|t| Instant::now() > t.until) {
            self.toast = None;
        }
        if let Some(finished) = self.pump_job() {
            self.job_finished(finished);
        }
    }

    /// Забирає вивід завдання; дзвонить, коли скрипт щось питає. Some — завдання щойно завершилося.
    fn pump_job(&mut self) -> Option<Finished> {
        let run = self.run.as_mut()?;
        run.job.pump();
        let prompt = if run.job.running() { run.job.analyzer.prompt(Instant::now()) } else { None };
        if prompt.is_some() && prompt != run.last_prompt {
            bell();
        }
        run.last_prompt = prompt;
        let exit = run.job.exit.filter(|_| !run.finished_handled)?;
        run.finished_handled = true;
        Some(Finished {
            success: exit.success,
            cancelled: run.job.analyzer.cancelled.is_some() || run.interrupted,
            elapsed: run.job.elapsed(),
            action: run.action,
        })
    }

    /// Завдання завершилося: про довге — дзвінок і сповіщення на робочому столі (людина могла
    /// відійти), а відомості про систему й звіти — наново.
    fn job_finished(&mut self, f: Finished) {
        if f.elapsed > LONG_TASK {
            bell();
            let label = menu_item(f.action).label();
            if f.success {
                desktop::notify("OBS-Legacy", &trf!("{label}: готово ✓", "{label}: done ✓"));
            } else if !f.cancelled {
                desktop::notify(
                    "OBS-Legacy",
                    &trf!(
                        "{label}: помилка — що робити, підкаже OBS-Legacy",
                        "{label}: error — OBS-Legacy will tell you what to do"
                    ),
                );
            }
        }
        // Після завершення курсор у меню знову ставиться на пораду.
        self.menu_touched = false;
        self.refresh();
    }

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toast = Some(Toast { text: text.into(), kind, until: Instant::now() + TOAST_FOR });
    }

    fn go_home(&mut self) {
        self.screen = Screen::Home;
        self.editor = None;
        self.settings_error = None;
    }
}
