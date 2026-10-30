//! Стан програми, головний цикл і обробка клавіатури та миші.

use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};

use crate::advice::{self, Advice};
use crate::analyzer::Prompt;
use crate::catalog::{self, PluginInfo};
use crate::desktop;
use crate::reports::{self, Outcome, ReportInfo};
use crate::runner::Job;
use crate::script;
use crate::service;
use crate::settings::{self, Kind, Settings};
use crate::system::{self, BuildInfo, SysInfo, Updates};
use crate::theme::Theme;
use crate::ui;
use crate::updates;

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
pub enum Action {
    Check,
    Deps,
    Install,
    Rebuild,
    Launch,
    Plugins,
    Status,
    Notify,
    Reports,
    Rollback,
    Settings,
    SelfUpdate,
    Clean,
    Uninstall,
    Help,
    Quit,
}

pub struct MenuItem {
    pub action: Action,
    pub icon: &'static str,
    pub label: &'static str,
    pub about: &'static str,
    pub sudo: bool,
    pub changes_system: bool,
    pub duration: &'static str,
    /// У розділі «Додатково» (згорнутий, поки його не відкрити).
    pub advanced: bool,
}

pub const MENU: &[MenuItem] = &[
    MenuItem {
        action: Action::Check,
        icon: "◎",
        label: "Перевірити систему",
        about: "Перевіряє драйвер, NVENC і пакети, завантажує вихідні коди OBS і CEF. Нічого не встановлює \
                і не змінює.",
        sudo: false,
        changes_system: false,
        duration: "5–15 хв (уперше довше)",
        advanced: false,
    },
    MenuItem {
        action: Action::Install,
        icon: "▼",
        label: "Встановити / оновити OBS",
        about: "Збирає найновіший OBS з NVENC, Browser Source, вибраними плагінами й ffmpeg саме під цей \
                комп'ютер, перевіряє NVENC і ставить пакети pacman (-legacy-580xx) у /usr. Попередній набір \
                лишається для відкату.",
        sudo: true,
        changes_system: true,
        duration: "30–90 хв, повторне — швидше",
        advanced: false,
    },
    MenuItem {
        action: Action::Launch,
        icon: "▶",
        label: "Запустити OBS",
        about: "Відкриває OBS з NVENC — те саме, що ярлик OBS у меню програм або команда obs.",
        sudo: false,
        changes_system: false,
        duration: "",
        advanced: false,
    },
    MenuItem {
        action: Action::Plugins,
        icon: "⊕",
        label: "Плагіни",
        about: "Галочками обери, що зібрати разом з OBS. obs-multi-rtmp (стрім на кілька платформ) увімкнено \
                типово. Плагіни оновлюються разом з OBS.",
        sudo: false,
        changes_system: false,
        duration: "",
        advanced: false,
    },
    MenuItem {
        action: Action::Status,
        icon: "≡",
        label: "Стан і оновлення",
        about: "Докладний звіт: активна збірка, плагіни, бібліотеки, нові версії OBS і плагінів.",
        sudo: false,
        changes_system: false,
        duration: "до хвилини",
        advanced: true,
    },
    MenuItem {
        action: Action::Notify,
        icon: "◷",
        label: "Сповіщення про оновлення",
        about: "Раз на 12 годин у фоні перевіряє нові версії OBS, плагінів і самого OBS-Legacy, а після оновлень \
                системи — чи власному OBS не бракує бібліотек. Про новини — сповіщення на робочому столі.",
        sudo: false,
        changes_system: false,
        duration: "секунди",
        advanced: true,
    },
    MenuItem {
        action: Action::Reports,
        icon: "◫",
        label: "Звіти",
        about: "Усі попередні запуски: що вийшло, що пішло не так і що з цим робити. Звіт можна відкрити тут, \
                у програмі для тексту або скопіювати шлях.",
        sudo: false,
        changes_system: false,
        duration: "",
        advanced: true,
    },
    MenuItem {
        action: Action::Rebuild,
        icon: "⊚",
        label: "Перезібрати OBS",
        about: "Збирає OBS і плагіни начисто (кеш збірки очищується) і ставить знову — навіть якщо нового \
                немає. Коли потрібно: збирання обірвалося (вимкнулося світло, закрилося вікно), OBS \
                поводиться дивно чи падає.",
        sudo: true,
        changes_system: true,
        duration: "30–90 хв",
        advanced: true,
    },
    MenuItem {
        action: Action::Rollback,
        icon: "↩",
        label: "Відкат на іншу збірку",
        about: "Ставить попередній набір пакетів OBS-Legacy через pacman, якщо нова версія поводиться гірше. \
                Без перезбирання.",
        sudo: true,
        changes_system: true,
        duration: "до хвилини",
        advanced: true,
    },
    MenuItem {
        action: Action::Settings,
        icon: "◈",
        label: "Налаштування",
        about: "Версія OBS, під який процесор збирати, чи збирати ffmpeg, скільки наборів тримати, фон \
                інтерфейсу та параметри для досвідчених. Діють і для скрипта без TUI.",
        sudo: false,
        changes_system: false,
        duration: "",
        advanced: true,
    },
    MenuItem {
        action: Action::SelfUpdate,
        icon: "↪",
        label: "Оновити OBS-Legacy",
        about: "Завантажує нову версію самої програми з GitHub (git pull) і перевстановлює її. Твій OBS \
                не зміниться. Після оновлення перезапусти програму.",
        sudo: false,
        changes_system: false,
        duration: "1–3 хв",
        advanced: true,
    },
    MenuItem {
        action: Action::Deps,
        icon: "⊞",
        label: "Встановити пакети",
        about: "Ставить через pacman лише пакети, потрібні для збірки OBS і вибраних плагінів (pacman сам \
                додасть їхні залежності). Повне оновлення системи не запускається.",
        sudo: true,
        changes_system: true,
        duration: "1–5 хв",
        advanced: true,
    },
    MenuItem {
        action: Action::Clean,
        icon: "⌫",
        label: "Очистити кеш збірки",
        about: "Видаляє проміжні файли збірки (кілька ГБ). Завантажені вихідні коди, архів CEF і набори \
                пакетів для відкату лишаються.",
        sudo: false,
        changes_system: false,
        duration: "секунди",
        advanced: true,
    },
    MenuItem {
        action: Action::Uninstall,
        icon: "✗",
        label: "Видалити OBS-Legacy",
        about: "Прибирає пакети OBS-Legacy (pacman -R; ffmpeg замінює системним з репозиторію) і залишки \
                старої схеми в /opt. Налаштування OBS і кеш збірки не чіпає.",
        sudo: true,
        changes_system: true,
        duration: "секунди",
        advanced: true,
    },
    MenuItem {
        action: Action::Help,
        icon: "?",
        label: "Довідка",
        about: "Як усе працює, клавіші, де лежать файли, що робити, якщо щось пішло не так, і хто це зробив.",
        sudo: false,
        changes_system: false,
        duration: "",
        advanced: false,
    },
    MenuItem {
        action: Action::Quit,
        icon: "×",
        label: "Вихід",
        about: "Закрити OBS-Legacy. Зібраний OBS лишається встановленим і запускається як завжди.",
        sudo: false,
        changes_system: false,
        duration: "",
        advanced: false,
    },
];

/// Дія TUI для «[ВИПРАВИТИ] команда: …» зі скрипта. Лише дозволені команди.
pub fn fix_action(command: &str) -> Option<Action> {
    match command {
        "deps" => Some(Action::Deps),
        "clean" => Some(Action::Clean),
        "check" => Some(Action::Check),
        "rebuild" => Some(Action::Rebuild),
        _ => None,
    }
}

pub fn menu_item(action: Action) -> &'static MenuItem {
    MENU.iter().find(|m| m.action == action).unwrap_or(&MENU[0])
}

/// Рядок головного меню: пункт або заголовок розділу «Додатково».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Item(Action),
    More,
}

/// Елементи, на які можна клацнути мишею (заповнюються під час малювання).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Menu(usize),
    Plugin(usize),
    Build(usize),
    Setting(usize),
    Report(usize),
    DialogOk,
    DialogCancel,
    DialogAlt,
    Continue,
    Back,
    Quit,
    OpenReport,
    CopyPath,
    OpenExternal,
    AdviceAction,
    AdviceAlt,
    RunPrimary,
    ToggleDetails,
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

#[derive(Clone, Debug)]
pub enum DialogAction {
    Run(Action),
    /// Збирання OBS: встановити, оновити (лише якщо є що) чи перезібрати начисто.
    Build(BuildMode),
    LaunchSystemObs,
    /// Увімкнути (true) чи вимкнути сповіщення про оновлення.
    Notifier(bool),
}

/// Як збирати OBS — команда скрипта.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildMode {
    Install,
    Update,
    Rebuild,
}

impl BuildMode {
    pub fn command(self) -> &'static str {
        match self {
            BuildMode::Install => "install",
            BuildMode::Update => "update",
            BuildMode::Rebuild => "rebuild",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            BuildMode::Install => "Встановити OBS",
            BuildMode::Update => "Оновити OBS",
            BuildMode::Rebuild => "Перезібрати OBS",
        }
    }

    fn action(self) -> Action {
        if self == BuildMode::Rebuild { Action::Rebuild } else { Action::Install }
    }
}

pub struct Dialog {
    pub title: String,
    pub lines: Vec<String>,
    pub confirm: &'static str,
    pub danger: bool,
    /// Друга кнопка (клавіша R): підпис і дія.
    pub alt: Option<(&'static str, DialogAction)>,
    pub action: DialogAction,
}

enum Msg {
    Sys(Box<SysInfo>),
    Updates(Updates),
    Catalog(Result<Vec<PluginInfo>, String>),
    Reports(Vec<ReportInfo>),
    /// Пакети для збірки, яких бракує: (є в репозиторіях, немає в репозиторіях).
    Deps(Option<(Vec<String>, Vec<String>)>),
    /// Сповіщення про оновлення: Some(служба) — увімкнено.
    Notifier(Option<service::Backend>),
}

pub struct RunState {
    pub job: Job,
    pub action: Action,
    pub title: String,
    /// Зсув прокрутки (рядків від низу).
    pub scroll: usize,
    pub output_area: Rect,
    ctrl_c_at: Option<Instant>,
    pub interrupted: bool,
    last_prompt: Option<Prompt>,
    finished_handled: bool,
    /// «Що далі» на картці успіху, якщо не типове для дії.
    pub next: Option<String>,
}

/// Вбудований переглядач звіту.
pub struct ReportView {
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub error: Option<String>,
    /// Перший видимий рядок (після перенесення).
    pub scroll: usize,
    /// Висота видимої частини (заповнюється під час малювання).
    pub page: usize,
    /// Перенесені рядки для поточної ширини: (ширина, [(номер рядка звіту, текст)]).
    pub wrapped: Option<(u16, Vec<(usize, String)>)>,
    /// Рядок звіту, до якого прокрутити під час наступного малювання.
    pub jump: Option<usize>,
    /// Куди повертатися (Esc).
    pub back: Screen,
}

impl ReportView {
    pub fn open(path: PathBuf, back: Screen) -> Self {
        let (lines, error) = match reports::load_lines(&path) {
            Ok(l) => (l, None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        // Одразу показати останню помилку (а якщо її немає — останнє попередження).
        let kinds: Vec<_> = lines.iter().map(|l| reports::classify(l)).collect();
        let jump = kinds
            .iter()
            .rposition(|k| *k == reports::LineKind::Error)
            .or_else(|| kinds.iter().rposition(|k| k.is_problem()));
        Self { path, lines, error, scroll: 0, page: 1, wrapped: None, jump, back }
    }

    fn rows(&self) -> usize {
        self.wrapped.as_ref().map_or(self.lines.len(), |(_, r)| r.len())
    }

    pub fn max_scroll(&self) -> usize {
        self.rows().saturating_sub(self.page)
    }

    fn scroll_by(&mut self, delta: isize) {
        self.scroll = self.scroll.saturating_add_signed(delta).min(self.max_scroll());
    }

    /// Перейти до наступного (або попереднього) попередження чи помилки.
    fn jump_problem(&mut self, forward: bool) -> bool {
        let Some((_, rows)) = &self.wrapped else { return false };
        let starts: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(i, (n, _))| (*i == 0 || rows[i - 1].0 != *n) && reports::classify(&self.lines[*n]).is_problem())
            .map(|(i, _)| i)
            .collect();
        // Позиція «фокуса» — третина екрана від верху.
        let focus = self.scroll + self.page / 3;
        let target = if forward {
            starts.into_iter().find(|&r| r > focus)
        } else {
            starts.into_iter().rev().find(|&r| r < focus)
        };
        match target {
            Some(r) => {
                self.scroll = r.saturating_sub(self.page / 3).min(self.max_scroll());
                true
            }
            None => false,
        }
    }
}

/// Простий однорядковий редактор для полів налаштувань.
pub struct Editor {
    pub chars: Vec<char>,
    pub cursor: usize,
}

impl Editor {
    fn new(s: &str) -> Self {
        let chars: Vec<char> = s.chars().collect();
        let cursor = chars.len();
        Self { chars, cursor }
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    fn key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                self.chars.insert(self.cursor, c);
                self.cursor += 1;
            }
            KeyCode::Char('u') => {
                self.chars.drain(..self.cursor);
                self.cursor = 0;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.chars.remove(self.cursor);
            }
            KeyCode::Delete if self.cursor < self.chars.len() => {
                self.chars.remove(self.cursor);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.chars.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.chars.len(),
            _ => {}
        }
    }
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
    pub self_update_src: Option<PathBuf>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    quit: bool,
}

fn ctrl(k: &KeyEvent, c: char) -> bool {
    k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char(x) if x.eq_ignore_ascii_case(&c))
}

/// Клавіша → байти для псевдотермінала.
pub fn key_to_bytes(k: &KeyEvent) -> Option<Vec<u8>> {
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    let mut out = match k.code {
        KeyCode::Char(c) if k.modifiers.contains(KeyModifiers::CONTROL) => {
            let b = c.to_ascii_lowercase() as u32;
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

fn notify_desktop(title: &str, body: &str) {
    let _ = Command::new("notify-send")
        .args(["-a", "OBS-Legacy", title, body])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn bell() {
    let mut out = std::io::stdout();
    let _ = out.write_all(b"\x07");
    let _ = out.flush();
}

/// «~/x» → «$HOME/x»; інші шляхи — як є.
pub fn expand_home(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => script::home().join(rest),
        None => PathBuf::from(p),
    }
}

/// Шлях для показу: домашня тека — як «~».
pub fn short_path(p: &std::path::Path) -> String {
    let home = script::home();
    match p.strip_prefix(&home) {
        Ok(rest) if home.as_os_str() != "/" => format!("~/{}", rest.display()),
        _ => p.display().to_string(),
    }
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
            self_update_src: script::source_dir(),
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
            let timeout = Duration::from_millis(if busy { 60 } else { 200 });
            if event::poll(timeout)? {
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

    // ---------------------------------------------------------------- меню

    /// Рядки головного меню: основні кроки, «Додатково» (з пунктами, якщо
    /// розгорнуто), довідка й вихід.
    pub fn rows(&self) -> Vec<Row> {
        let promoted = self.deps_promoted();
        let visible = |m: &&MenuItem| {
            (m.action != Action::SelfUpdate || self.self_update_src.is_some())
                && !(promoted && m.action == Action::Deps)
        };
        let mut rows: Vec<Row> = Vec::new();
        for m in MENU.iter().filter(|m| !m.advanced && !matches!(m.action, Action::Help | Action::Quit)) {
            rows.push(Row::Item(m.action));
            // Коли бракує пакетів, їхнє встановлення — окремий крок одразу після перевірки.
            if m.action == Action::Check && promoted {
                rows.push(Row::Item(Action::Deps));
            }
        }
        rows.push(Row::More);
        if self.advanced_open {
            rows.extend(MENU.iter().filter(|m| m.advanced).filter(visible).map(|m| Row::Item(m.action)));
        }
        rows.push(Row::Item(Action::Help));
        rows.push(Row::Item(Action::Quit));
        rows
    }

    /// Чи показувати «Встановити пакети для збірки» серед основних кроків.
    pub fn deps_promoted(&self) -> bool {
        self.missing_packages.as_ref().is_some_and(|m| !m.is_empty())
    }

    fn select_action(&mut self, action: Action) {
        let item = menu_item(action);
        if item.advanced && !(action == Action::Deps && self.deps_promoted()) {
            self.advanced_open = true;
        }
        if let Some(i) = self.rows().iter().position(|r| *r == Row::Item(action)) {
            self.menu_sel = i;
        }
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
            self_update: self.self_update_src.is_some(),
            notifier: self.notifier.map(|n| n.is_some()),
            build_running: self.build_running(),
        }))
    }

    /// Нові версії OBS, плагінів і OBS-Legacy — так само, як їх бачать сповіщення.
    pub fn facts(&self) -> Option<updates::Facts> {
        let u = self.updates.as_ref()?;
        let active = self.sys.as_ref().and_then(SysInfo::active_build);
        let catalog = self.catalog.as_deref().unwrap_or_default();
        Some(updates::facts_from(active, u, &self.settings, catalog, self.self_update_src.is_some()))
    }

    /// Поки людина не рухала вибір, курсор стоїть на рекомендованому пункті.
    fn preselect(&mut self) {
        if self.menu_touched || !self.reports_loaded {
            return;
        }
        if let Some(action) = self.advice().and_then(|a| a.action) {
            self.select_action(action);
        }
    }

    // ---------------------------------------------------------------- фон

    fn spawn_sys(&mut self) {
        if self.sys_loading {
            return;
        }
        self.sys_loading = true;
        let tx = self.tx.clone();
        let exe = self.exe.clone();
        let tag = self.settings.get("NV_HEADERS_TAG");
        thread::spawn(move || {
            let info = system::gather(&exe, &tag);
            let _ = tx.send(Msg::Sys(Box::new(info)));
            let _ = tx.send(Msg::Notifier(service::installed().first().map(|p| p.backend)));
        });
    }

    fn spawn_reports(&mut self) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let _ = tx.send(Msg::Reports(reports::list()));
        });
    }

    /// Яких пакетів для збірки бракує (`deps --missing`, без sudo і швидко).
    fn spawn_deps(&mut self) {
        let tx = self.tx.clone();
        let script = self.script.clone();
        thread::spawn(move || {
            let out = Command::new("bash")
                .arg(&script)
                .args(["deps", "--missing"])
                .env("OBS_LEGACY_SETTINGS", settings::settings_path())
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output();
            let parsed = out.ok().filter(|o| o.status.success()).map(|o| {
                let (mut repo, mut norepo) = (Vec::new(), Vec::new());
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    match line.split_once(' ') {
                        Some(("repo", p)) => repo.push(p.trim().to_string()),
                        Some(("norepo", p)) => norepo.push(p.trim().to_string()),
                        _ => {}
                    }
                }
                (repo, norepo)
            });
            let _ = tx.send(Msg::Deps(parsed));
        });
    }

    fn spawn_catalog(&mut self) {
        let tx = self.tx.clone();
        let script = self.script.clone();
        thread::spawn(move || {
            let r = catalog::load(&script).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Catalog(r));
        });
    }

    fn spawn_updates(&mut self) {
        let (Some(sys), Some(cat)) = (&self.sys, &self.catalog) else { return };
        self.updates_requested = true;
        self.updates_loading = true;
        let mut ids: Vec<String> =
            sys.active_build().map(|b| b.plugins().into_iter().map(|(id, _)| id).collect()).unwrap_or_default();
        for id in self.settings.plugins() {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let repos: Vec<(String, String)> =
            ids.into_iter().filter_map(|id| cat.iter().find(|p| p.id == id).map(|p| (id, p.repo.clone()))).collect();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let self_repo = updates::self_repo();
            let _ = tx.send(Msg::Updates(system::check_updates(&repos, Some(&self_repo))));
        });
    }

    fn refresh(&mut self) {
        self.spawn_sys();
        self.spawn_reports();
        self.spawn_deps();
        self.updates = None;
        self.updates_requested = false;
    }

    fn process_messages(&mut self) {
        let mut changed = false;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Sys(info) => {
                    self.sys = Some(*info);
                    self.sys_loading = false;
                    changed = true;
                }
                Msg::Updates(u) => {
                    self.updates = Some(u);
                    self.updates_loading = false;
                    changed = true;
                }
                Msg::Catalog(Ok(list)) => {
                    self.catalog = Some(list);
                    self.catalog_error = None;
                }
                Msg::Catalog(Err(e)) => self.catalog_error = Some(e),
                Msg::Reports(list) => {
                    self.reports = list;
                    self.reports_loaded = true;
                    self.reports_sel = self.reports_sel.min(self.reports.len().saturating_sub(1));
                    changed = true;
                }
                Msg::Deps(Some((repo, norepo))) => {
                    self.missing_packages = Some(repo);
                    self.missing_norepo = norepo;
                    changed = true;
                }
                Msg::Deps(None) => {}
                Msg::Notifier(n) => {
                    self.notifier = Some(n);
                    changed = true;
                }
            }
        }
        if changed {
            self.preselect();
        }
        if !self.updates_requested && self.screen != Screen::Intro && self.sys.is_some() && self.catalog.is_some() {
            self.spawn_updates();
        }
    }

    fn update(&mut self) {
        let now = Instant::now();
        if self.toast.as_ref().is_some_and(|t| now > t.until) {
            self.toast = None;
        }
        let mut finished: Option<(bool, bool, Duration, Action)> = None;
        if let Some(run) = self.run.as_mut() {
            run.job.pump();
            let prompt = if run.job.running() { run.job.analyzer.prompt(now) } else { None };
            if prompt.is_some() && prompt != run.last_prompt {
                bell();
            }
            run.last_prompt = prompt;
            if let Some(exit) = run.job.exit
                && !run.finished_handled
            {
                run.finished_handled = true;
                let cancelled = run.job.analyzer.cancelled.is_some() || run.interrupted;
                finished = Some((exit.success, cancelled, run.job.elapsed(), run.action));
            }
        }
        if let Some((success, cancelled, elapsed, action)) = finished {
            if elapsed > Duration::from_secs(60) {
                bell();
                let label = menu_item(action).label;
                if success {
                    notify_desktop("OBS-Legacy", &format!("{label}: готово ✓"));
                } else if !cancelled {
                    notify_desktop("OBS-Legacy", &format!("{label}: помилка — що робити, підкаже OBS-Legacy"));
                }
            }
            // Після завершення курсор у меню знову ставиться на пораду.
            self.menu_touched = false;
            self.refresh();
        }
    }

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toast = Some(Toast { text: text.into(), kind, until: Instant::now() + Duration::from_secs(5) });
    }

    // ---------------------------------------------------------------- дії

    fn go_home(&mut self) {
        self.screen = Screen::Home;
        self.editor = None;
        self.settings_error = None;
    }

    fn activate_row(&mut self, row: Row) {
        match row {
            Row::Item(a) => self.activate(a),
            Row::More => {
                self.advanced_open = !self.advanced_open;
                if !self.advanced_open {
                    self.menu_sel = self.rows().iter().position(|r| *r == Row::More).unwrap_or(0);
                }
            }
        }
    }

    fn activate(&mut self, action: Action) {
        match action {
            Action::Check => {
                self.start_script(action, &["check"]);
            }
            Action::Install => self.install_dialog(),
            Action::Rebuild => self.rebuild_dialog(),
            Action::Status => {
                self.start_script(action, &["status"]);
            }
            Action::Notify => self.notifier_dialog(),
            Action::Deps => {
                let missing = self.missing_packages.clone().unwrap_or_default();
                let what = if missing.is_empty() {
                    "Скрипт перевірить, чи всі пакети для збірки на місці, і поставить ті, яких бракує.".to_string()
                } else {
                    format!("Буде встановлено: {}.", missing.join(" "))
                };
                self.dialog = Some(Dialog {
                    title: "Встановити пакети для збірки?".into(),
                    lines: vec![
                        what,
                        String::new(),
                        "Лише ці пакети через pacman -S --needed (залежності pacman додасть сам), без оновлення \
                         системи."
                            .into(),
                        "Скрипт ще раз спитає y/n, а пароль sudo/doas вводь прямо у вікні виводу.".into(),
                    ],
                    confirm: "Встановити",
                    danger: false,
                    alt: None,
                    action: DialogAction::Run(Action::Deps),
                });
            }
            Action::Plugins => {
                if self.catalog.is_none() {
                    self.spawn_catalog();
                }
                self.screen = Screen::Plugins;
            }
            Action::Reports => {
                self.spawn_reports();
                self.reports_sel = 0;
                self.screen = Screen::Reports;
            }
            Action::Rollback => {
                let (builds, _) = system::read_builds();
                self.build_sel = builds.iter().position(|b| !b.active && b.complete).unwrap_or(0);
                self.builds = builds;
                self.screen = Screen::Rollback;
            }
            Action::Launch => self.launch_obs(false),
            Action::Settings => {
                self.settings = Settings::load();
                self.screen = Screen::Settings;
            }
            Action::Clean => {
                self.start_script(action, &["clean"]);
            }
            Action::Uninstall => {
                self.dialog = Some(Dialog {
                    title: "Видалити OBS-Legacy?".into(),
                    lines: vec![
                        "Буде прибрано пакети *-legacy-580xx (OBS і плагіни) через pacman -R, а \
                         ffmpeg-legacy-580xx заміниться системним ffmpeg з репозиторію. Залишки старої схеми \
                         (/opt/obs-studio-nvenc, /usr/local/bin/obs) теж приберуться."
                            .into(),
                        "Налаштування OBS (~/.config/obs-studio), кеш збірки й набори пакетів лишаться. \
                         Звичайний OBS потім: sudo pacman -S obs-studio."
                            .into(),
                        String::new(),
                        "Скрипт ще раз запитає підтвердження у вікні виводу.".into(),
                    ],
                    confirm: "Продовжити",
                    danger: true,
                    alt: None,
                    action: DialogAction::Run(Action::Uninstall),
                });
            }
            Action::SelfUpdate => {
                if let Some(dir) = self.self_update_src.clone() {
                    self.dialog = Some(Dialog {
                        title: "Оновити OBS-Legacy?".into(),
                        lines: vec![
                            format!("git pull у {}", short_path(&dir)),
                            "і перевстановлення програми (cargo build). Твій OBS і налаштування не зміняться.".into(),
                        ],
                        confirm: "Оновити",
                        danger: false,
                        alt: None,
                        action: DialogAction::Run(Action::SelfUpdate),
                    });
                }
            }
            Action::Help => {
                self.help_scroll = 0;
                self.screen = Screen::Help;
            }
            Action::Quit => self.quit = true,
        }
    }

    fn confirm_dialog(&mut self) {
        let Some(dialog) = self.dialog.take() else { return };
        self.run_dialog_action(dialog.action);
    }

    /// Друга кнопка діалогу (R).
    fn alt_dialog(&mut self) {
        let Some(dialog) = self.dialog.take() else { return };
        if let Some((_, action)) = dialog.alt {
            self.run_dialog_action(action);
        }
    }

    fn run_dialog_action(&mut self, action: DialogAction) {
        match action {
            DialogAction::Build(mode) => {
                if self.start_script(mode.action(), &[mode.command()])
                    && let Some(run) = self.run.as_mut()
                {
                    run.title = mode.title().into();
                }
            }
            DialogAction::Run(Action::Uninstall) => {
                self.start_script(Action::Uninstall, &["uninstall"]);
            }
            DialogAction::Run(Action::Deps) => {
                self.start_script(Action::Deps, &["deps"]);
            }
            DialogAction::Run(Action::SelfUpdate) => {
                if let Some(dir) = self.self_update_src.clone() {
                    let args = vec![
                        "-c".to_string(),
                        "set -e; cd \"$1\"; echo \"==> git pull ($1)\"; git pull --ff-only; \
                         echo '==> Перевстановлення OBS-Legacy'; exec ./install.sh --update"
                            .to_string(),
                        "obs-legacy-update".to_string(),
                        dir.display().to_string(),
                    ];
                    self.start_job(Action::SelfUpdate, "bash", args);
                }
            }
            DialogAction::Run(other) => self.activate(other),
            DialogAction::LaunchSystemObs => self.launch_obs(true),
            DialogAction::Notifier(enable) => {
                let args = vec!["--notifier".to_string(), if enable { "enable" } else { "disable" }.to_string()];
                let exe = self.exe.display().to_string();
                if self.start_job(Action::Notify, &exe, args)
                    && let Some(run) = self.run.as_mut()
                {
                    run.title = if enable {
                        "Увімкнути сповіщення"
                    } else {
                        "Вимкнути сповіщення"
                    }
                    .into();
                    run.next = Some(
                        if enable {
                            "Перевірка працює у фоні — OBS-Legacy можна закрити. Про нові версії OBS і плагінів \
                             прийде сповіщення."
                        } else {
                            "Сповіщень більше не буде. Увімкнути знову: «Додатково → Сповіщення про оновлення»."
                        }
                        .into(),
                    );
                }
            }
        }
    }

    /// Чи йде зараз збирання: власне завдання або скрипт, запущений деінде (тримає блокування кешу).
    /// Блокування перевіряється лише тоді, коли останній звіт збирання без результату — щоб не
    /// відкривати файл на кожному кадрі.
    fn build_running(&self) -> bool {
        self.run.as_ref().is_some_and(|r| r.job.running())
            || (self
                .reports
                .iter()
                .find(|r| matches!(r.command.as_str(), "install" | "update" | "rebuild"))
                .is_some_and(|r| r.outcome == Outcome::Unknown)
                && system::build_running())
    }

    /// Останнє встановлення / оновлення / перезбирання обірвалося (звіт без «Результат:», а скрипт уже
    /// не працює) або завершилося помилкою.
    pub fn last_build_broken(&self) -> bool {
        let running = self.build_running();
        self.reports
            .iter()
            .find(|r| matches!(r.command.as_str(), "install" | "update" | "rebuild"))
            .is_some_and(|r| r.outcome == Outcome::Failed || (r.outcome == Outcome::Unknown && !running))
    }

    /// «Встановити / оновити OBS»: якщо OBS-Legacy уже стоїть — типово «Оновити» (спершу перевірка, що
    /// нового), а друга кнопка — «Перезібрати» начисто.
    fn install_dialog(&mut self) {
        let plugins = self.settings.plugins();
        let obs_ref = self.settings.get("OBS_REF");
        let plugins_line =
            format!("Плагіни: {}", if plugins.is_empty() { "без плагінів".to_string() } else { plugins.join(", ") });
        let ffmpeg_line = if self.settings.get("BUILD_FFMPEG") == "0" {
            "ffmpeg: системний (збирання вимкнено в Налаштуваннях)".to_string()
        } else {
            "ffmpeg: власний ffmpeg-legacy-580xx з NVENC (перезбирається, лише коли в репозиторії вийшов новий \
             ffmpeg)"
                .to_string()
        };
        let pacman_line = "Пакети *-legacy-580xx ставляться через pacman у /usr. pacman сам спитає, чи замінити \
                           obs-studio (чи obs-studio-git), ffmpeg і ті самі плагіни з AUR, — відповідай y.";
        let time_line = "Збирання триває від 30 хв до години й більше (уперше з ffmpeg — довше). Пароль sudo/doas \
                         і відповіді y/n вводь прямо у вікні виводу — програма підкаже, коли.";
        let installed =
            self.sys.as_ref().and_then(SysInfo::active_build).map(|b| (b.obs_version().to_string(), b.built()));
        let Some((version, built)) = installed else {
            let version =
                if obs_ref == "stable" { "найновіша стабільна".to_string() } else { obs_ref };
            // Минуле збирання обірвалося чи впало — можна одразу начисто.
            let alt =
                self.last_build_broken().then_some(("Перезібрати начисто", DialogAction::Build(BuildMode::Rebuild)));
            self.dialog = Some(Dialog {
                title: "Зібрати і встановити OBS?".into(),
                lines: vec![
                    format!("Версія OBS: {version}"),
                    plugins_line,
                    ffmpeg_line,
                    String::new(),
                    pacman_line.into(),
                    time_line.into(),
                    "Поточний OBS не зміниться, доки нова збірка не пройде всі перевірки.".into(),
                ],
                confirm: "Почати",
                danger: false,
                alt,
                action: DialogAction::Build(BuildMode::Install),
            });
            return;
        };
        let mut lines = vec![if built.is_empty() {
            format!("Встановлено: OBS {version}")
        } else {
            format!("Встановлено: OBS {version} (зібрано {built})")
        }];
        let facts = self.facts();
        match facts.as_ref() {
            Some(f) if f.obs_update().is_some() => {
                lines.push(format!("Найновіший: OBS {} — його й буде зібрано.", f.obs_update().unwrap_or_default()))
            }
            Some(f) if f.obs_latest.is_some() && f.obs_pinned.is_none() => lines.push(format!(
                "OBS {version} — уже найновіший. «Оновити» перевірить плагіни й ffmpeg і збиратиме, лише якщо є що \
                 оновлювати."
            )),
            _ => lines.push(
                "«Оновити» спершу перевірить, що нового (OBS, плагіни, ffmpeg), і збиратиме, лише якщо є що \
                 оновлювати."
                    .into(),
            ),
        }
        if obs_ref != "stable" {
            lines.push(format!("Версію OBS зафіксовано: {obs_ref} (Налаштування → «Версія OBS»)."));
        }
        if let Some(f) = facts.as_ref()
            && !f.plugin_updates.is_empty()
        {
            lines.push(format!("Нові версії плагінів: {}.", updates::plugin_list(&f.plugin_updates)));
        }
        lines.push(plugins_line);
        lines.push(ffmpeg_line);
        lines.push(String::new());
        lines.push(
            "«Перезібрати» (R) — зібрати все начисто й поставити знову, навіть якщо нового немає: коли минуле \
             збирання обірвалося (вимкнулося світло, закрилося вікно) чи OBS поводиться дивно."
                .into(),
        );
        lines.push(pacman_line.into());
        self.dialog = Some(Dialog {
            title: "Оновити OBS?".into(),
            lines,
            confirm: "Оновити",
            danger: false,
            alt: Some(("Перезібрати", DialogAction::Build(BuildMode::Rebuild))),
            action: DialogAction::Build(BuildMode::Update),
        });
    }

    /// «Перезібрати OBS»: усе начисто, навіть якщо нового немає.
    fn rebuild_dialog(&mut self) {
        let mut lines = Vec::new();
        if let Some(b) = self.sys.as_ref().and_then(SysInfo::active_build) {
            lines.push(format!(
                "Встановлено: OBS {}. Версії — як для оновлення: найновіші, якщо в Налаштуваннях не зафіксовано \
                 інших.",
                b.obs_version()
            ));
        }
        lines.push(
            "Збере OBS і плагіни з нуля: кеш збірки OBS очищується, застарілі блокування git після обірваного \
             збирання прибираються. Потім — ті самі перевірки NVENC і бібліотек і встановлення через pacman."
                .into(),
        );
        lines.push(String::new());
        lines.push(
            "Коли потрібно: збирання обірвалося (вимкнулося світло, закрилося вікно), OBS поводиться дивно чи \
             падає. Для звичайного оновлення досить «Оновити»."
                .into(),
        );
        lines.push("Довше, ніж оновлення: 30–90 хв, бо все компілюється з нуля.".into());
        self.dialog = Some(Dialog {
            title: "Перезібрати OBS начисто?".into(),
            lines,
            confirm: "Перезібрати",
            danger: false,
            alt: None,
            action: DialogAction::Build(BuildMode::Rebuild),
        });
    }

    /// Діалог «Сповіщення про оновлення»: що це, чи ввімкнено, увімкнути чи вимкнути.
    fn notifier_dialog(&mut self) {
        let st = service::status();
        let mut lines = vec![
            "Раз на 12 годин OBS-Legacy у фоні перевіряє, чи вийшли нові версії OBS, плагінів і самої \
             програми, а після оновлень системи — чи власному OBS не бракує бібліотек. Про кожну новину — одне \
             сповіщення на робочому столі; клацання по ньому відкриває OBS-Legacy."
                .to_string(),
            String::new(),
        ];
        let (title, confirm, enable) = match st.installed.first() {
            Some(p) => {
                let state = if st.running.is_some() { "працює" } else { "зараз не працює" };
                lines.push(format!("Увімкнено: {} ({}), {state}.", p.backend.label(), p.shown()));
                lines.extend(st.last_line().map(|l| format!("{l}.")));
                ("Сповіщення про оновлення", "Вимкнути", false)
            }
            None => {
                if let Some(best) = service::available().first() {
                    lines.push(format!(
                        "Перевірку запускатиме {} — це підходить для твоєї системи. OBS-Legacy можна закривати.",
                        best.backend.label()
                    ));
                }
                lines.push("Мережа потрібна лише для коротких запитів до GitHub (git ls-remote).".into());
                ("Увімкнути сповіщення про оновлення?", "Увімкнути", true)
            }
        };
        self.dialog = Some(Dialog {
            title: title.into(),
            lines,
            confirm,
            danger: false,
            alt: None,
            action: DialogAction::Notifier(enable),
        });
    }

    /// Запустити команду скрипта. false — не запущено.
    fn start_script(&mut self, action: Action, args: &[&str]) -> bool {
        let mut full = vec![self.script.display().to_string()];
        full.extend(args.iter().map(|s| s.to_string()));
        self.start_job(action, "bash", full)
    }

    /// Запустити завдання у вбудованому терміналі. false — не запущено.
    fn start_job(&mut self, action: Action, program: &str, args: Vec<String>) -> bool {
        if self.run.as_ref().is_some_and(|r| r.job.running()) {
            self.screen = Screen::Run;
            self.toast(ToastKind::Warn, "Спершу дочекайся завершення поточного завдання.");
            return false;
        }
        let (cols, rows) = ratatui::crossterm::terminal::size().unwrap_or((120, 36));
        let env = [
            ("OBS_LEGACY_TUI", "1".to_string()),
            ("OBS_LEGACY_SETTINGS", settings::settings_path().display().to_string()),
        ];
        match Job::spawn(
            program,
            &args,
            &script::home(),
            &env,
            rows.saturating_sub(9).max(5),
            cols.saturating_sub(4).max(20),
        ) {
            Ok(job) => {
                let item = menu_item(action);
                self.run = Some(RunState {
                    job,
                    action,
                    title: item.label.to_string(),
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
                self.toast(ToastKind::Err, format!("Не вдалося запустити: {e:#}"));
                false
            }
        }
    }

    /// `anyway` — запустити навіть системний OBS, якщо власної збірки немає.
    fn launch_obs(&mut self, anyway: bool) {
        let installed = self.sys.as_ref().is_some_and(|s| s.active_build().is_some());
        if !installed && !anyway {
            self.dialog = Some(Dialog {
                title: "OBS-Legacy ще не встановлено".into(),
                lines: vec![
                    "Зараз команда obs запустить системний OBS — без NVENC на драйвері 580xx.".into(),
                    "Щоб отримати OBS з NVENC, спершу обери «Встановити / оновити OBS».".into(),
                ],
                confirm: "Усе одно запустити",
                danger: false,
                alt: None,
                action: DialogAction::LaunchSystemObs,
            });
            return;
        }
        let Some(obs) = system::find_in_path("obs") else {
            self.toast(ToastKind::Err, "Команду obs не знайдено — спершу «Встановити / оновити OBS».");
            return;
        };
        let mut cmd = Command::new(&obs);
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        // SAFETY: setsid лише відокремлює OBS від сесії термінала.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        match cmd.spawn() {
            Ok(_) => self.toast(ToastKind::Ok, format!("OBS запускається ({})…", obs.display())),
            Err(e) => self.toast(ToastKind::Err, format!("Не вдалося запустити OBS: {e}")),
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
    fn run_primary_activate(&mut self) {
        let primary = self.run_primary();
        self.go_home();
        if let Some(action) = primary {
            self.select_action(action);
            self.activate(action);
        }
    }

    // ---------------------------------------------------------------- звіти

    fn open_report(&mut self, path: PathBuf, back: Screen) {
        self.report_view = Some(ReportView::open(path, back));
        self.screen = Screen::Report;
    }

    /// Шлях до звіту, з яким зараз працює екран.
    fn current_report(&self) -> Option<PathBuf> {
        match self.screen {
            Screen::Report => self.report_view.as_ref().map(|v| v.path.clone()),
            Screen::Reports => self.reports.get(self.reports_sel).map(|r| r.path.clone()),
            Screen::Run => self.run.as_ref().and_then(|r| r.job.analyzer.report.as_deref()).map(expand_home),
            Screen::Home => {
                self.advice().and_then(|a| a.report).or_else(|| self.reports.first().map(|r| r.path.clone()))
            }
            _ => None,
        }
    }

    fn copy_report_path(&mut self) {
        let Some(path) = self.current_report() else {
            self.toast(ToastKind::Info, "Звіту ще немає.");
            return;
        };
        let text = path.display().to_string();
        match desktop::copy(&text) {
            Ok(()) => self.toast(ToastKind::Ok, format!("Шлях скопійовано: {}", short_path(&path))),
            Err(e) => self.toast(ToastKind::Err, format!("Не вдалося скопіювати: {e:#}")),
        }
    }

    fn open_report_externally(&mut self, folder: bool) {
        let Some(path) = self.current_report() else {
            self.toast(ToastKind::Info, "Звіту ще немає.");
            return;
        };
        let target = if folder { path.parent().map(PathBuf::from).unwrap_or(path) } else { path };
        match desktop::open(&target) {
            Ok(()) => self.toast(ToastKind::Ok, format!("Відкриваю {}…", short_path(&target))),
            Err(e) => self.toast(ToastKind::Err, format!("Не вдалося відкрити: {e:#}")),
        }
    }

    /// «o»: переглянути звіт у самій програмі.
    fn view_current_report(&mut self) {
        match self.current_report() {
            Some(path) => self.open_report(path, self.screen),
            None => self.toast(ToastKind::Info, "Звіту ще немає."),
        }
    }

    // ---------------------------------------------------------------- події

    fn handle_event(&mut self, ev: Event) {
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
            for c in text.chars().filter(|c| !c.is_control()) {
                ed.chars.insert(ed.cursor, c);
                ed.cursor += 1;
            }
        }
    }

    fn on_key(&mut self, k: KeyEvent) {
        if self.dialog.is_some() {
            match k.code {
                KeyCode::Enter => self.confirm_dialog(),
                KeyCode::Char('y' | 'Y' | 'т' | 'Т') => self.confirm_dialog(),
                KeyCode::Char('r' | 'R' | 'к' | 'К') if self.dialog.as_ref().is_some_and(|d| d.alt.is_some()) => {
                    self.alt_dialog()
                }
                KeyCode::Esc | KeyCode::Char('n' | 'N' | 'q' | 'н' | 'Н') => self.dialog = None,
                _ => {}
            }
            return;
        }
        if self.screen != Screen::Run && ctrl(&k, 'c') {
            self.quit = true;
            return;
        }
        match self.screen {
            Screen::Intro => match k.code {
                KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right => self.go_home(),
                KeyCode::Esc | KeyCode::Char('q' | 'й') => self.quit = true,
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
        match k.code {
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => self.menu_sel = (self.menu_sel + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => self.menu_sel = (self.menu_sel + 1) % n,
            KeyCode::Home | KeyCode::Char('g') => self.menu_sel = 0,
            KeyCode::End | KeyCode::Char('G') => self.menu_sel = n - 1,
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l' | ' ') => self.activate_row(rows[self.menu_sel]),
            KeyCode::Left | KeyCode::Char('h') if self.advanced_open => {
                self.advanced_open = false;
                self.menu_sel = self.rows().iter().position(|r| *r == Row::More).unwrap_or(0);
            }
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < n && rows[i] != Row::More {
                    self.menu_sel = i;
                }
            }
            KeyCode::Char('o' | 'щ') => self.view_current_report(),
            KeyCode::Char('d' | 'в') => self.details = !self.details,
            KeyCode::Char('?') | KeyCode::F(1) => self.activate(Action::Help),
            KeyCode::Char('r' | 'к') | KeyCode::F(5) => {
                self.refresh();
                self.toast(ToastKind::Info, "Оновлюю відомості…");
                touched = false;
            }
            // Esc спершу згортає «Додатково», а вже потім закриває програму.
            KeyCode::Esc if self.advanced_open => {
                self.advanced_open = false;
                self.menu_sel = self.rows().iter().position(|r| *r == Row::More).unwrap_or(0);
            }
            KeyCode::Esc | KeyCode::Char('q' | 'й') => self.quit = true,
            _ => touched = false,
        }
        self.menu_touched |= touched;
    }

    fn run_key(&mut self, k: KeyEvent) {
        let Some(run) = self.run.as_mut() else {
            self.go_home();
            return;
        };
        let page = usize::from(run.output_area.height.max(2) - 1);
        match k.code {
            KeyCode::PageUp => {
                run.scroll += page;
                return;
            }
            KeyCode::PageDown => {
                run.scroll = run.scroll.saturating_sub(page);
                return;
            }
            KeyCode::Up if k.modifiers.contains(KeyModifiers::SHIFT) => {
                run.scroll += 1;
                return;
            }
            KeyCode::Down if k.modifiers.contains(KeyModifiers::SHIFT) => {
                run.scroll = run.scroll.saturating_sub(1);
                return;
            }
            KeyCode::End if k.modifiers.contains(KeyModifiers::CONTROL) || run.scroll > 0 => {
                run.scroll = 0;
                return;
            }
            _ => {}
        }
        if !run.job.running() {
            match k.code {
                KeyCode::Enter => self.run_primary_activate(),
                KeyCode::Esc | KeyCode::Char('q' | 'й') | KeyCode::Backspace => self.go_home(),
                KeyCode::Up => run.scroll += 1,
                KeyCode::Down => run.scroll = run.scroll.saturating_sub(1),
                KeyCode::Home => run.scroll = usize::MAX / 2,
                KeyCode::Char('o' | 'щ') => self.view_current_report(),
                KeyCode::Char('c' | 'с') => self.copy_report_path(),
                KeyCode::Char('e' | 'у') => self.open_report_externally(false),
                _ => {}
            }
            return;
        }
        if ctrl(&k, 'c') {
            let now = Instant::now();
            let armed = run.ctrl_c_at.is_some_and(|t| now.duration_since(t) < Duration::from_secs(3));
            if armed && run.interrupted {
                run.job.kill();
                run.ctrl_c_at = None;
                self.toast(ToastKind::Warn, "Завдання примусово зупинено.");
            } else if armed {
                run.job.interrupt();
                run.interrupted = true;
                run.ctrl_c_at = Some(now);
                self.toast(ToastKind::Warn, "Надіслано Ctrl+C. Якщо скрипт не зупиниться — ще двічі Ctrl+C.");
            } else {
                run.ctrl_c_at = Some(now);
                self.toast(ToastKind::Warn, "Натисни Ctrl+C ще раз, щоб перервати завдання.");
            }
            return;
        }
        // Скрипт питає лише y/N і пароль. Стрілки, Home/End, Esc йому не потрібні, а в рядку
        // введення лишилися б сміттям — і «y» на запитанні вже не зараховувалося б.
        match k.code {
            KeyCode::Up => run.scroll += 1,
            KeyCode::Down => run.scroll = run.scroll.saturating_sub(1),
            KeyCode::Home => run.scroll = usize::MAX / 2,
            KeyCode::End => run.scroll = 0,
            KeyCode::Esc => self.toast(ToastKind::Info, "Завдання ще працює. Перервати — Ctrl+C."),
            KeyCode::Left | KeyCode::Right | KeyCode::Delete | KeyCode::Insert | KeyCode::BackTab => {}
            _ => {
                if let Some(bytes) = key_to_bytes(&k) {
                    run.scroll = 0;
                    run.job.write_input(&bytes);
                }
            }
        }
    }

    fn plugins_key(&mut self, k: KeyEvent) {
        let n = self.catalog.as_ref().map_or(0, Vec::len);
        match k.code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('q' | 'й') | KeyCode::Backspace => self.go_home(),
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

    fn toggle_plugin(&mut self, i: usize) {
        let Some(id) = self.catalog.as_ref().and_then(|c| c.get(i)).map(|p| p.id.clone()) else { return };
        let mut ids = self.settings.plugins();
        if let Some(pos) = ids.iter().position(|x| *x == id) {
            ids.remove(pos);
        } else {
            ids.push(id);
        }
        self.save_plugins(ids);
    }

    fn set_plugins(&mut self, pred: impl Fn(&PluginInfo) -> bool) {
        let ids = self
            .catalog
            .as_ref()
            .map(|c| c.iter().filter(|p| pred(p)).map(|p| p.id.clone()).collect())
            .unwrap_or_default();
        self.save_plugins(ids);
    }

    fn save_plugins(&mut self, ids: Vec<String>) {
        // Порядок як у каталозі — так зручніше читати settings.env.
        let ordered: Vec<String> = match &self.catalog {
            Some(cat) => cat.iter().filter(|p| ids.contains(&p.id)).map(|p| p.id.clone()).collect(),
            None => ids,
        };
        self.settings.set_plugins(&ordered);
        match self.settings.save() {
            Ok(()) => self.toast(ToastKind::Ok, "Збережено — застосується при наступному «Встановити / оновити OBS»."),
            Err(e) => self.toast(ToastKind::Err, format!("Не вдалося зберегти налаштування: {e}")),
        }
    }

    fn rollback_key(&mut self, k: KeyEvent) {
        let n = self.builds.len();
        match k.code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('q' | 'й') | KeyCode::Backspace => self.go_home(),
            KeyCode::Up | KeyCode::Char('k') if n > 0 => self.build_sel = (self.build_sel + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') if n > 0 => self.build_sel = (self.build_sel + 1) % n,
            KeyCode::Enter if n > 0 => self.rollback_to(self.build_sel),
            _ => {}
        }
    }

    fn rollback_to(&mut self, i: usize) {
        let Some(b) = self.builds.get(i) else { return };
        if b.active {
            self.toast(ToastKind::Info, "Ця збірка вже активна.");
        } else if !b.complete {
            self.toast(ToastKind::Warn, "Ця збірка неповна — на неї не можна перемкнутися.");
        } else {
            let label = b.label.clone();
            self.start_script(Action::Rollback, &["rollback", &label]);
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
        match k.code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('q' | 'й') | KeyCode::Backspace => self.go_home(),
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

    fn edit_setting(&mut self, i: usize) {
        let f = &settings::FIELDS[i];
        if f.kind == Kind::Toggle {
            let new = if self.settings.get(f.key) == "1" { "0" } else { "1" };
            self.commit_setting(i, new);
        } else if let Some(next) = f.kind.next_choice(&self.settings.get(f.key)) {
            self.commit_setting(i, next);
        } else {
            self.editor = Some(Editor::new(&self.settings.get(f.key)));
            self.settings_error = None;
        }
    }

    fn commit_setting(&mut self, i: usize, value: &str) {
        let f = &settings::FIELDS[i];
        if let Err(e) = settings::validate(f, value) {
            self.settings_error = Some(e);
            return;
        }
        self.settings.set(f.key, value);
        match self.settings.save() {
            Ok(()) => {
                self.editor = None;
                self.settings_error = None;
                self.toast(ToastKind::Ok, format!("{}: збережено", f.label));
                match f.key {
                    "NV_HEADERS_TAG" => self.refresh(),
                    "UI_BACKGROUND" => self.theme = Theme::detect(self.settings.solid_background()),
                    _ => {}
                }
            }
            Err(e) => self.settings_error = Some(format!("не вдалося зберегти: {e}")),
        }
    }

    fn reports_key(&mut self, k: KeyEvent) {
        let n = self.reports.len();
        match k.code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('q' | 'й') | KeyCode::Backspace => self.go_home(),
            KeyCode::Up | KeyCode::Char('k') if n > 0 => self.reports_sel = (self.reports_sel + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') if n > 0 => self.reports_sel = (self.reports_sel + 1) % n,
            KeyCode::Home => self.reports_sel = 0,
            KeyCode::End if n > 0 => self.reports_sel = n - 1,
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('o' | 'щ') if n > 0 => {
                let path = self.reports[self.reports_sel].path.clone();
                self.open_report(path, Screen::Reports);
            }
            KeyCode::Char('c' | 'с') => self.copy_report_path(),
            KeyCode::Char('e' | 'у') => self.open_report_externally(false),
            KeyCode::Char('f' | 'а') => self.open_report_externally(true),
            _ => {}
        }
    }

    fn report_key(&mut self, k: KeyEvent) {
        let Some(view) = self.report_view.as_mut() else {
            self.go_home();
            return;
        };
        let page = view.page.max(2) as isize - 1;
        match k.code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('q' | 'й') | KeyCode::Backspace => {
                let back = view.back;
                if back == Screen::Home {
                    self.go_home();
                } else {
                    self.screen = back;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => view.scroll_by(-1),
            KeyCode::Down | KeyCode::Char('j') => view.scroll_by(1),
            KeyCode::PageUp => view.scroll_by(-page),
            KeyCode::PageDown | KeyCode::Char(' ') => view.scroll_by(page),
            KeyCode::Home | KeyCode::Char('g') => view.scroll = 0,
            KeyCode::End | KeyCode::Char('G') => view.scroll = view.max_scroll(),
            KeyCode::Char(c @ ('n' | 'p' | 'т' | 'з')) => {
                if !view.jump_problem(matches!(c, 'n' | 'т')) {
                    self.toast(ToastKind::Info, "Більше попереджень і помилок у цьому напрямку немає.");
                }
            }
            KeyCode::Char('c' | 'с') => self.copy_report_path(),
            KeyCode::Char('e' | 'у') => self.open_report_externally(false),
            KeyCode::Char('f' | 'а') => self.open_report_externally(true),
            _ => {}
        }
    }

    fn help_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('q' | 'й') | KeyCode::Backspace => self.go_home(),
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

    fn target_at(&self, x: u16, y: u16) -> Option<Target> {
        let pos = Position { x, y };
        self.hits.iter().rev().find(|(r, _)| r.contains(pos)).map(|(_, t)| *t)
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        match m.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let up = m.kind == MouseEventKind::ScrollUp;
                self.on_wheel(up);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(t) = self.target_at(m.column, m.row) {
                    self.on_click(t);
                }
            }
            _ => {}
        }
    }

    fn on_wheel(&mut self, up: bool) {
        let step = |v: usize, n: usize| if up { (v + n - 1) % n } else { (v + 1) % n };
        match self.screen {
            Screen::Run => {
                if let Some(run) = self.run.as_mut() {
                    run.scroll = if up { run.scroll + 3 } else { run.scroll.saturating_sub(3) };
                }
            }
            Screen::Home if self.dialog.is_none() => {
                self.menu_sel = step(self.menu_sel, self.rows().len());
                self.menu_touched = true;
            }
            Screen::Plugins => {
                let n = self.catalog.as_ref().map_or(0, Vec::len);
                if n > 0 {
                    self.plugin_sel = step(self.plugin_sel, n);
                }
            }
            Screen::Rollback if !self.builds.is_empty() => self.build_sel = step(self.build_sel, self.builds.len()),
            Screen::Settings if self.editor.is_none() => {
                self.settings_sel = step(self.settings_sel, settings::FIELDS.len())
            }
            Screen::Reports if !self.reports.is_empty() => {
                self.reports_sel = step(self.reports_sel, self.reports.len())
            }
            Screen::Report => {
                if let Some(v) = self.report_view.as_mut() {
                    v.scroll_by(if up { -3 } else { 3 });
                }
            }
            Screen::Help => {
                self.help_scroll = if up {
                    self.help_scroll.saturating_sub(3)
                } else {
                    (self.help_scroll + 3).min(self.help_max_scroll)
                }
            }
            _ => {}
        }
    }

    fn on_click(&mut self, t: Target) {
        match t {
            Target::DialogOk => self.confirm_dialog(),
            Target::DialogCancel => self.dialog = None,
            Target::DialogAlt => self.alt_dialog(),
            Target::Quit => self.quit = true,
            _ if self.dialog.is_some() => {}
            Target::Continue => match self.screen {
                Screen::Intro => self.go_home(),
                Screen::Run if self.run.as_ref().is_some_and(|r| !r.job.running()) => self.go_home(),
                _ => {}
            },
            Target::Back => match self.screen {
                Screen::Run if self.run.as_ref().is_some_and(|r| r.job.running()) => {}
                Screen::Report => {
                    let back = self.report_view.as_ref().map_or(Screen::Home, |v| v.back);
                    if back == Screen::Home { self.go_home() } else { self.screen = back }
                }
                _ => self.go_home(),
            },
            // Клац по вибраному пункту — запуск; по іншому — вибір.
            Target::Menu(i) => {
                let rows = self.rows();
                if let Some(row) = rows.get(i).copied() {
                    self.menu_touched = true;
                    if self.menu_sel == i || row == Row::More {
                        self.menu_sel = i;
                        self.activate_row(row);
                    } else {
                        self.menu_sel = i;
                    }
                }
            }
            Target::Plugin(i) => {
                self.plugin_sel = i;
                self.toggle_plugin(i);
            }
            Target::Build(i) => {
                if self.build_sel == i {
                    self.rollback_to(i);
                } else {
                    self.build_sel = i;
                }
            }
            Target::Setting(i) if self.editor.is_none() => {
                if self.settings_sel == i {
                    self.edit_setting(i);
                } else {
                    self.settings_sel = i;
                }
            }
            Target::Report(i) => {
                if self.reports_sel == i {
                    if let Some(r) = self.reports.get(i) {
                        let path = r.path.clone();
                        self.open_report(path, Screen::Reports);
                    }
                } else {
                    self.reports_sel = i;
                }
            }
            Target::OpenReport => self.view_current_report(),
            Target::CopyPath => self.copy_report_path(),
            Target::OpenExternal => self.open_report_externally(false),
            Target::AdviceAction | Target::AdviceAlt => {
                let advice = self.advice();
                let action = advice.and_then(|a| if t == Target::AdviceAlt { a.alt } else { a.action });
                if let Some(action) = action {
                    self.menu_touched = true;
                    self.select_action(action);
                    self.activate(action);
                }
            }
            Target::RunPrimary => self.run_primary_activate(),
            Target::ToggleDetails => self.details = !self.details,
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

    #[test]
    fn editor_basics() {
        let mut e = Editor::new("33.0");
        e.key(key(KeyCode::Char('.'), KeyModifiers::NONE));
        e.key(key(KeyCode::Char('1'), KeyModifiers::NONE));
        assert_eq!(e.text(), "33.0.1");
        e.key(key(KeyCode::Home, KeyModifiers::NONE));
        e.key(key(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(e.text(), "3.0.1");
        e.key(key(KeyCode::End, KeyModifiers::NONE));
        e.key(key(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(e.text(), "3.0.");
    }

    #[test]
    fn home_paths() {
        let home = script::home();
        assert_eq!(expand_home("~/.cache/r.txt"), home.join(".cache/r.txt"));
        assert_eq!(expand_home("/tmp/r.txt"), PathBuf::from("/tmp/r.txt"));
        if home.as_os_str() != "/" {
            assert_eq!(short_path(&home.join(".cache/r.txt")), "~/.cache/r.txt");
        }
    }

    #[test]
    fn menu_has_unique_actions() {
        for (i, a) in MENU.iter().enumerate() {
            assert!(MENU.iter().skip(i + 1).all(|b| b.action != a.action));
        }
    }

    #[test]
    fn beginner_menu_is_short_and_expands() {
        let mut app = App::new(PathBuf::from("/bin/true"), PathBuf::from("/bin/true"));
        app.self_update_src = None;
        let collapsed = app.rows();
        // Чотири кроки, «Додатково», довідка й вихід.
        assert_eq!(collapsed.len(), 7);
        assert_eq!(collapsed[0], Row::Item(Action::Check));
        assert!(collapsed.contains(&Row::More));
        assert!(!collapsed.contains(&Row::Item(Action::Uninstall)));
        app.select_action(Action::Uninstall);
        assert!(app.advanced_open);
        assert_eq!(app.rows()[app.menu_sel], Row::Item(Action::Uninstall));
        assert!(!app.rows().contains(&Row::Item(Action::SelfUpdate)));
    }
}
