//! Головне меню: пункти (`MENU`), рядки з розділом «Додатково» і що робить кожен пункт.

use super::dialogs::{Dialog, DialogAction};
use super::{App, Screen};
use crate::paths::short_path;
use crate::script::SelfSource;
use crate::settings::Settings;
use crate::system;

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
    /// Назва, опис і тривалість: українською, англійською (див. `label()`, `about()`, `duration()`).
    pub label: (&'static str, &'static str),
    pub about: (&'static str, &'static str),
    pub sudo: bool,
    pub changes_system: bool,
    pub duration: (&'static str, &'static str),
    /// У розділі «Додатково» (згорнутий, поки його не відкрити).
    pub advanced: bool,
}

impl MenuItem {
    pub fn label(&self) -> &'static str {
        tr!(self.label.0, self.label.1)
    }

    pub fn about(&self) -> &'static str {
        tr!(self.about.0, self.about.1)
    }

    pub fn duration(&self) -> &'static str {
        tr!(self.duration.0, self.duration.1)
    }
}

pub const MENU: &[MenuItem] = &[
    MenuItem {
        action: Action::Check,
        icon: "◎",
        label: ("Перевірити систему", "Check system"),
        about: (
            "Перевіряє драйвер, NVENC і пакети, завантажує вихідні коди OBS і CEF. Нічого не встановлює \
             і не змінює.",
            "Checks the driver, NVENC and packages, downloads the OBS and CEF sources. Installs and changes \
             nothing.",
        ),
        sudo: false,
        changes_system: false,
        duration: ("5–15 хв (уперше довше)", "5–15 min (longer the first time)"),
        advanced: false,
    },
    MenuItem {
        action: Action::Install,
        icon: "▼",
        label: ("Встановити / оновити OBS", "Install / update OBS"),
        about: (
            "Збирає найновіший OBS з NVENC, Browser Source, вибраними плагінами й ffmpeg саме під цей \
             комп'ютер, перевіряє NVENC і ставить пакети pacman (-legacy-580xx) у /usr. Попередній набір \
             лишається для відкату.",
            "Builds the latest OBS with NVENC, Browser Source, the selected plugins and ffmpeg for exactly \
             this computer, checks NVENC and installs pacman packages (-legacy-580xx) into /usr. The previous \
             set stays for rollback.",
        ),
        sudo: true,
        changes_system: true,
        duration: ("30–90 хв, повторне — швидше", "30–90 min, faster next time"),
        advanced: false,
    },
    MenuItem {
        action: Action::Launch,
        icon: "▶",
        label: ("Запустити OBS", "Launch OBS"),
        about: (
            "Відкриває OBS з NVENC — те саме, що ярлик OBS у меню програм або команда obs.",
            "Opens OBS with NVENC — the same as the OBS entry in the application menu or the obs command.",
        ),
        sudo: false,
        changes_system: false,
        duration: ("", ""),
        advanced: false,
    },
    MenuItem {
        action: Action::Plugins,
        icon: "⊕",
        label: ("Плагіни", "Plugins"),
        about: (
            "Галочками обери, що зібрати разом з OBS. obs-multi-rtmp (стрім на кілька платформ) увімкнено \
             типово. Плагіни оновлюються разом з OBS.",
            "Tick what to build together with OBS. obs-multi-rtmp (streaming to several platforms) is on by \
             default. Plugins are updated together with OBS.",
        ),
        sudo: false,
        changes_system: false,
        duration: ("", ""),
        advanced: false,
    },
    MenuItem {
        action: Action::Status,
        icon: "≡",
        label: ("Стан і оновлення", "Status and updates"),
        about: (
            "Докладний звіт: активна збірка, плагіни, бібліотеки, нові версії OBS і плагінів.",
            "A detailed report: the active build, plugins, libraries, new versions of OBS and plugins.",
        ),
        sudo: false,
        changes_system: false,
        duration: ("до хвилини", "up to a minute"),
        advanced: true,
    },
    MenuItem {
        action: Action::Notify,
        icon: "◷",
        label: ("Сповіщення про оновлення", "Update notifications"),
        about: (
            "Раз на 12 годин у фоні перевіряє нові версії OBS, плагінів і самого OBS-Legacy, а після оновлень \
             системи — чи власному OBS не бракує бібліотек. Про новини — сповіщення на робочому столі.",
            "Every 12 hours checks in the background for new versions of OBS, plugins and OBS-Legacy itself, \
             and after system updates — whether your OBS is missing libraries. News arrive as desktop \
             notifications.",
        ),
        sudo: false,
        changes_system: false,
        duration: ("секунди", "seconds"),
        advanced: true,
    },
    MenuItem {
        action: Action::Reports,
        icon: "◫",
        label: ("Звіти", "Reports"),
        about: (
            "Усі попередні запуски: що вийшло, що пішло не так і що з цим робити. Звіт можна відкрити тут, \
             у програмі для тексту або скопіювати шлях.",
            "All previous runs: what worked, what went wrong and what to do about it. Open a report here, in a \
             text app, or copy its path.",
        ),
        sudo: false,
        changes_system: false,
        duration: ("", ""),
        advanced: true,
    },
    MenuItem {
        action: Action::Rebuild,
        icon: "⊚",
        label: ("Перезібрати OBS", "Rebuild OBS"),
        about: (
            "Збирає OBS і плагіни начисто (кеш збірки очищується) і ставить знову — навіть якщо нового \
             немає. Коли потрібно: збирання обірвалося (вимкнулося світло, закрилося вікно), OBS \
             поводиться дивно чи падає.",
            "Builds OBS and plugins from scratch (the build cache is cleared) and installs them again — even \
             if nothing is new. When needed: a build was cut off (power outage, closed window), or OBS behaves \
             oddly or crashes.",
        ),
        sudo: true,
        changes_system: true,
        duration: ("30–90 хв", "30–90 min"),
        advanced: true,
    },
    MenuItem {
        action: Action::Rollback,
        icon: "↩",
        label: ("Відкат на іншу збірку", "Roll back to another build"),
        about: (
            "Ставить попередній набір пакетів OBS-Legacy через pacman, якщо нова версія поводиться гірше. \
             Без перезбирання.",
            "Installs a previous set of OBS-Legacy packages with pacman if the new version behaves worse. \
             No rebuild.",
        ),
        sudo: true,
        changes_system: true,
        duration: ("до хвилини", "up to a minute"),
        advanced: true,
    },
    MenuItem {
        action: Action::Settings,
        icon: "◈",
        label: ("Налаштування", "Settings"),
        about: (
            "Версія OBS, під який процесор збирати, чи збирати ffmpeg, скільки наборів тримати, мова й фон \
             інтерфейсу та параметри для досвідчених. Діють і для скрипта без TUI.",
            "The OBS version, which CPU to tune for, whether to build ffmpeg, how many sets to keep, the \
             interface language and background, and options for advanced users. They apply to the script \
             without the TUI too.",
        ),
        sudo: false,
        changes_system: false,
        duration: ("", ""),
        advanced: true,
    },
    MenuItem {
        action: Action::SelfUpdate,
        icon: "↪",
        label: ("Оновити OBS-Legacy", "Update OBS-Legacy"),
        about: (
            "Завантажує нову версію самої програми з GitHub і ставить її: з вихідних кодів — git pull і \
             збирання, з релізу — готову програму. Твій OBS не зміниться. Після оновлення перезапусти програму.",
            "Downloads the new version of the program itself from GitHub and installs it: from source — git pull \
             and a build, from a release — the ready-made program. Your OBS stays as it is. Restart the program \
             afterwards.",
        ),
        sudo: false,
        changes_system: false,
        duration: ("1–3 хв", "1–3 min"),
        advanced: true,
    },
    MenuItem {
        action: Action::Deps,
        icon: "⊞",
        label: ("Встановити пакети", "Install packages"),
        about: (
            "Ставить через pacman лише пакети, потрібні для збірки OBS і вибраних плагінів (pacman сам \
             додасть їхні залежності). Повне оновлення системи не запускається.",
            "Installs with pacman only the packages needed to build OBS and the selected plugins (pacman adds \
             their dependencies itself). No full system update is run.",
        ),
        sudo: true,
        changes_system: true,
        duration: ("1–5 хв", "1–5 min"),
        advanced: true,
    },
    MenuItem {
        action: Action::Clean,
        icon: "⌫",
        label: ("Очистити кеш збірки", "Clean the build cache"),
        about: (
            "Видаляє проміжні файли збірки (кілька ГБ). Завантажені вихідні коди, архів CEF і набори \
             пакетів для відкату лишаються.",
            "Deletes intermediate build files (several GB). The downloaded sources, the CEF archive and the \
             package sets for rollback stay.",
        ),
        sudo: false,
        changes_system: false,
        duration: ("секунди", "seconds"),
        advanced: true,
    },
    MenuItem {
        action: Action::Uninstall,
        icon: "✗",
        label: ("Видалити OBS-Legacy", "Remove OBS-Legacy"),
        about: (
            "Прибирає пакети OBS-Legacy (pacman -R; ffmpeg замінює системним з репозиторію) і залишки \
             старої схеми в /opt. Налаштування OBS і кеш збірки не чіпає.",
            "Removes the OBS-Legacy packages (pacman -R; ffmpeg is replaced with the repository one) and \
             leftovers of the old /opt scheme. OBS settings and the build cache are left alone.",
        ),
        sudo: true,
        changes_system: true,
        duration: ("секунди", "seconds"),
        advanced: true,
    },
    MenuItem {
        action: Action::Help,
        icon: "?",
        label: ("Довідка", "Help"),
        about: (
            "Як усе працює, клавіші, де лежать файли, що робити, якщо щось пішло не так, і хто це зробив.",
            "How it all works, keys, where the files are, what to do if something goes wrong, and who made it.",
        ),
        sudo: false,
        changes_system: false,
        duration: ("", ""),
        advanced: false,
    },
    MenuItem {
        action: Action::Quit,
        icon: "×",
        label: ("Вихід", "Quit"),
        about: (
            "Закрити OBS-Legacy. Зібраний OBS лишається встановленим і запускається як завжди.",
            "Close OBS-Legacy. The built OBS stays installed and starts as usual.",
        ),
        sudo: false,
        changes_system: false,
        duration: ("", ""),
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

impl App {
    /// Рядки головного меню: основні кроки, «Додатково» (з пунктами, якщо
    /// розгорнуто), довідка й вихід.
    pub fn rows(&self) -> Vec<Row> {
        let promoted = self.deps_promoted();
        let visible = |m: &&MenuItem| {
            (m.action != Action::SelfUpdate || self.self_source.is_some()) && !(promoted && m.action == Action::Deps)
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

    pub(super) fn select_action(&mut self, action: Action) {
        let item = menu_item(action);
        if item.advanced && !(action == Action::Deps && self.deps_promoted()) {
            self.advanced_open = true;
        }
        if let Some(i) = self.rows().iter().position(|r| *r == Row::Item(action)) {
            self.menu_sel = i;
        }
    }

    /// Поки людина не рухала вибір, курсор стоїть на рекомендованому пункті.
    pub(super) fn preselect(&mut self) {
        if self.menu_touched || !self.reports_loaded {
            return;
        }
        if let Some(action) = self.advice().and_then(|a| a.action) {
            self.select_action(action);
        }
    }

    pub(super) fn activate_row(&mut self, row: Row) {
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

    pub(super) fn activate(&mut self, action: Action) {
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
                    tr!(
                        "Скрипт перевірить, чи всі пакети для збірки на місці, і поставить ті, яких бракує.",
                        "The script checks that all build dependencies are in place and installs the missing ones."
                    )
                    .to_string()
                } else {
                    let list = missing.join(" ");
                    trf!("Буде встановлено: {list}.", "To be installed: {list}.")
                };
                self.dialog = Some(Dialog {
                    title: tr!("Встановити пакети для збірки?", "Install the build dependencies?").into(),
                    lines: vec![
                        what,
                        String::new(),
                        tr!(
                            "Лише ці пакети через pacman -S --needed (залежності pacman додасть сам), без оновлення \
                             системи.",
                            "Only these packages, with pacman -S --needed (pacman adds the dependencies itself), \
                             without a system update."
                        )
                        .into(),
                        tr!(
                            "Скрипт ще раз спитає y/n, а пароль sudo/doas вводь прямо у вікні виводу.",
                            "The script asks y/n once more; type your sudo/doas password right in the output window."
                        )
                        .into(),
                    ],
                    confirm: tr!("Встановити", "Install"),
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
                    title: tr!("Видалити OBS-Legacy?", "Remove OBS-Legacy?").into(),
                    lines: vec![
                        tr!(
                            "Буде прибрано пакети *-legacy-580xx (OBS і плагіни) через pacman -R, а \
                             ffmpeg-legacy-580xx заміниться системним ffmpeg з репозиторію. Залишки старої схеми \
                             (/opt/obs-studio-nvenc, /usr/local/bin/obs) теж приберуться.",
                            "The *-legacy-580xx packages (OBS and plugins) are removed with pacman -R, and \
                             ffmpeg-legacy-580xx is replaced with the system ffmpeg from the repository. Leftovers of \
                             the old layout (/opt/obs-studio-nvenc, /usr/local/bin/obs) are removed too."
                        )
                        .into(),
                        tr!(
                            "Налаштування OBS (~/.config/obs-studio), кеш збірки й набори пакетів лишаться. \
                             Звичайний OBS потім: sudo pacman -S obs-studio.",
                            "OBS settings (~/.config/obs-studio), the build cache and the package sets stay. The \
                             regular OBS afterwards: sudo pacman -S obs-studio."
                        )
                        .into(),
                        String::new(),
                        tr!(
                            "Скрипт ще раз запитає підтвердження у вікні виводу.",
                            "The script asks for confirmation once more in the output window."
                        )
                        .into(),
                    ],
                    confirm: tr!("Продовжити", "Continue"),
                    danger: true,
                    alt: None,
                    action: DialogAction::Run(Action::Uninstall),
                });
            }
            Action::SelfUpdate => {
                let lines = match &self.self_source {
                    Some(SelfSource::Git(dir)) => vec![
                        trf!("git pull у {}", "git pull in {}", short_path(dir)),
                        tr!(
                            "і перевстановлення програми (cargo build). Твій OBS і налаштування не зміняться.",
                            "and reinstalling the program (cargo build). Your OBS and settings stay as they are."
                        )
                        .into(),
                    ],
                    Some(SelfSource::Release) => vec![
                        tr!(
                            "Готова програма з найновішого релізу на GitHub, без збирання.",
                            "The ready-made program from the latest release on GitHub, no build."
                        )
                        .into(),
                        tr!("Твій OBS і налаштування не зміняться.", "Your OBS and settings stay as they are.").into(),
                    ],
                    None => return,
                };
                self.dialog = Some(Dialog {
                    title: tr!("Оновити OBS-Legacy?", "Update OBS-Legacy?").into(),
                    lines,
                    confirm: tr!("Оновити", "Update"),
                    danger: false,
                    alt: None,
                    action: DialogAction::Run(Action::SelfUpdate),
                });
            }
            Action::Help => {
                self.help_scroll = 0;
                self.screen = Screen::Help;
            }
            Action::Quit => self.quit = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn menu_has_unique_actions() {
        for (i, a) in MENU.iter().enumerate() {
            assert!(MENU.iter().skip(i + 1).all(|b| b.action != a.action));
        }
    }

    #[test]
    fn beginner_menu_is_short_and_expands() {
        let mut app = App::new(PathBuf::from("/bin/true"), PathBuf::from("/bin/true"));
        app.self_source = None;
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

    #[test]
    fn release_install_updates_from_the_menu() {
        let mut app = App::new(PathBuf::from("/bin/true"), PathBuf::from("/bin/true"));
        app.self_source = Some(SelfSource::Release);
        app.select_action(Action::SelfUpdate);
        assert_eq!(app.rows()[app.menu_sel], Row::Item(Action::SelfUpdate));
        app.activate(Action::SelfUpdate);
        let dialog = app.dialog.take().expect("діалог оновлення");
        assert!(dialog.lines[0].contains("релізу"), "{:?}", dialog.lines);
        app.self_source = None;
        assert!(!app.rows().contains(&Row::Item(Action::SelfUpdate)));
    }
}
