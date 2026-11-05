//! Діалоги підтвердження перед діями, що змінюють систему чи тривають довго: що саме буде, скільки
//! це займе і що ще спитає скрипт.

use super::App;
use super::jobs::self_update_args;
use super::menu::Action;
use crate::paths::short_path;
use crate::script::SelfSource;
use crate::service;
use crate::system::SysInfo;
use crate::updates;

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
            BuildMode::Install => tr!("Встановити OBS", "Install OBS"),
            BuildMode::Update => tr!("Оновити OBS", "Update OBS"),
            BuildMode::Rebuild => tr!("Перезібрати OBS", "Rebuild OBS"),
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

/// Як ставляться пакети — і що відповідати pacman.
fn pacman_line() -> &'static str {
    tr!(
        "Пакети *-legacy-580xx ставляться через pacman у /usr. pacman сам спитає, чи замінити \
         obs-studio (чи obs-studio-git), ffmpeg і ті самі плагіни з AUR, — відповідай y.",
        "The *-legacy-580xx packages are installed with pacman into /usr. pacman asks itself whether to replace \
         obs-studio (or obs-studio-git), ffmpeg and the same plugins from the AUR — answer y."
    )
}

/// Скільки триває збирання і де вводити пароль.
fn time_line() -> &'static str {
    tr!(
        "Збирання триває від 30 хв до години й більше (уперше з ffmpeg — довше). Пароль sudo/doas \
         і відповіді y/n вводь прямо у вікні виводу — програма підкаже, коли.",
        "The build takes from 30 minutes to an hour or more (longer the first time, with ffmpeg). Type your \
         sudo/doas password and y/n answers right in the output window — the program tells you when."
    )
}

impl App {
    pub(super) fn confirm_dialog(&mut self) {
        let Some(dialog) = self.dialog.take() else { return };
        self.run_dialog_action(dialog.action);
    }

    /// Друга кнопка діалогу (R).
    pub(super) fn alt_dialog(&mut self) {
        let Some(dialog) = self.dialog.take() else { return };
        if let Some((_, action)) = dialog.alt {
            self.run_dialog_action(action);
        }
    }

    pub(super) fn run_dialog_action(&mut self, action: DialogAction) {
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
                if let Some(src) = self.self_source.clone() {
                    self.start_job(Action::SelfUpdate, "bash", self_update_args(&src));
                }
            }
            DialogAction::Run(other) => self.activate(other),
            DialogAction::LaunchSystemObs => self.launch_obs(true),
            DialogAction::Notifier(enable) => self.switch_notifier(enable),
        }
    }

    /// `obs-legacy --notifier enable|disable` у вбудованому терміналі.
    fn switch_notifier(&mut self, enable: bool) {
        let args = vec!["--notifier".to_string(), if enable { "enable" } else { "disable" }.to_string()];
        let exe = self.exe.display().to_string();
        if !self.start_job(Action::Notify, &exe, args) {
            return;
        }
        let Some(run) = self.run.as_mut() else { return };
        run.title = if enable {
            tr!("Увімкнути сповіщення", "Turn on notifications")
        } else {
            tr!("Вимкнути сповіщення", "Turn off notifications")
        }
        .into();
        run.next = Some(
            if enable {
                tr!(
                    "Перевірка працює у фоні — OBS-Legacy можна закрити. Про нові версії OBS і плагінів \
                     прийде сповіщення.",
                    "The check runs in the background — you can close OBS-Legacy. New versions of OBS \
                     and plugins arrive as notifications."
                )
            } else {
                tr!(
                    "Сповіщень більше не буде. Увімкнути знову: «Додатково → Сповіщення про оновлення».",
                    "No more notifications. To turn them back on: “More → Update notifications”."
                )
            }
            .into(),
        );
    }

    /// «Встановити / оновити OBS»: якщо OBS-Legacy уже стоїть — типово «Оновити» (спершу перевірка, що
    /// нового), а друга кнопка — «Перезібрати» начисто.
    pub(super) fn install_dialog(&mut self) {
        let installed =
            self.sys.as_ref().and_then(SysInfo::active_build).map(|b| (b.obs_version().to_string(), b.built()));
        self.dialog = Some(match installed {
            Some((version, built)) => self.update_dialog(&version, &built),
            None => self.fresh_install_dialog(),
        });
    }

    /// Що збиратиметься разом з OBS: плагіни й ffmpeg.
    fn build_plan_lines(&self) -> [String; 2] {
        let plugins = self.settings.plugins();
        let list = if plugins.is_empty() {
            tr!("без плагінів", "no plugins").to_string()
        } else {
            plugins.join(", ")
        };
        let ffmpeg = if self.settings.get("BUILD_FFMPEG") == "0" {
            tr!(
                "ffmpeg: системний (збирання вимкнено в Налаштуваннях)",
                "ffmpeg: the system one (building is off in Settings)"
            )
        } else {
            tr!(
                "ffmpeg: власний ffmpeg-legacy-580xx з NVENC (перезбирається, лише коли в репозиторії вийшов новий \
                 ffmpeg)",
                "ffmpeg: our own ffmpeg-legacy-580xx with NVENC (rebuilt only when the repository has a new ffmpeg)"
            )
        };
        [trf!("Плагіни: {list}", "Plugins: {list}"), ffmpeg.to_string()]
    }

    /// Перше встановлення: OBS-Legacy ще немає.
    fn fresh_install_dialog(&self) -> Dialog {
        let obs_ref = self.settings.get("OBS_REF");
        let version = if obs_ref == "stable" {
            tr!("найновіша стабільна", "the latest stable").to_string()
        } else {
            obs_ref
        };
        // Минуле збирання обірвалося чи впало — можна одразу начисто.
        let alt = self
            .last_build_broken()
            .then_some((tr!("Перезібрати начисто", "Rebuild from scratch"), DialogAction::Build(BuildMode::Rebuild)));
        let [plugins_line, ffmpeg_line] = self.build_plan_lines();
        Dialog {
            title: tr!("Зібрати і встановити OBS?", "Build and install OBS?").into(),
            lines: vec![
                trf!("Версія OBS: {version}", "OBS version: {version}"),
                plugins_line,
                ffmpeg_line,
                String::new(),
                pacman_line().into(),
                time_line().into(),
                tr!(
                    "Поточний OBS не зміниться, доки нова збірка не пройде всі перевірки.",
                    "Your current OBS doesn't change until the new build passes every check."
                )
                .into(),
            ],
            confirm: tr!("Почати", "Start"),
            danger: false,
            alt,
            action: DialogAction::Build(BuildMode::Install),
        }
    }

    /// Оновлення встановленого OBS-Legacy: що нового, а друга кнопка — «Перезібрати» начисто.
    fn update_dialog(&self, version: &str, built: &str) -> Dialog {
        let obs_ref = self.settings.get("OBS_REF");
        let mut lines = vec![if built.is_empty() {
            trf!("Встановлено: OBS {version}", "Installed: OBS {version}")
        } else {
            trf!("Встановлено: OBS {version} (зібрано {built})", "Installed: OBS {version} (built {built})")
        }];
        let facts = self.facts();
        match facts.as_ref() {
            Some(f) if f.obs_update().is_some() => lines.push(trf!(
                "Найновіший: OBS {} — його й буде зібрано.",
                "The latest: OBS {} — that is what will be built.",
                f.obs_update().unwrap_or_default()
            )),
            Some(f) if f.obs_latest.is_some() && f.obs_pinned.is_none() => lines.push(trf!(
                "OBS {version} — уже найновіший. «Оновити» перевірить плагіни й ffmpeg і збиратиме, лише якщо є що \
                 оновлювати.",
                "OBS {version} is already the latest. “Update” checks plugins and ffmpeg and builds only if there \
                 is something to update."
            )),
            _ => lines.push(
                tr!(
                    "«Оновити» спершу перевірить, що нового (OBS, плагіни, ffmpeg), і збиратиме, лише якщо є що \
                     оновлювати.",
                    "“Update” first checks what is new (OBS, plugins, ffmpeg) and builds only if there is something \
                     to update."
                )
                .into(),
            ),
        }
        if obs_ref != "stable" {
            lines.push(trf!(
                "Версію OBS зафіксовано: {obs_ref} (Налаштування → «Версія OBS»).",
                "The OBS version is pinned: {obs_ref} (Settings → “OBS version”)."
            ));
        }
        if let Some(f) = facts.as_ref()
            && !f.plugin_updates.is_empty()
        {
            lines.push(trf!(
                "Нові версії плагінів: {}.",
                "New plugin versions: {}.",
                updates::plugin_list(&f.plugin_updates)
            ));
        }
        lines.extend(self.build_plan_lines());
        lines.push(String::new());
        lines.push(
            tr!(
                "«Перезібрати» (R) — зібрати все начисто й поставити знову, навіть якщо нового немає: коли минуле \
                 збирання обірвалося (вимкнулося світло, закрилося вікно) чи OBS поводиться дивно.",
                "“Rebuild” (R) — build everything from scratch and install it again, even if nothing is new: when \
                 the last build was cut off (power outage, closed window) or OBS behaves oddly."
            )
            .into(),
        );
        lines.push(pacman_line().into());
        Dialog {
            title: tr!("Оновити OBS?", "Update OBS?").into(),
            lines,
            confirm: tr!("Оновити", "Update"),
            danger: false,
            alt: Some((tr!("Перезібрати", "Rebuild"), DialogAction::Build(BuildMode::Rebuild))),
            action: DialogAction::Build(BuildMode::Update),
        }
    }

    /// «Перезібрати OBS»: усе начисто, навіть якщо нового немає.
    pub(super) fn rebuild_dialog(&mut self) {
        let mut lines = Vec::new();
        if let Some(b) = self.sys.as_ref().and_then(SysInfo::active_build) {
            lines.push(trf!(
                "Встановлено: OBS {}. Версії — як для оновлення: найновіші, якщо в Налаштуваннях не зафіксовано \
                 інших.",
                "Installed: OBS {}. Versions are as for an update: the latest, unless others are pinned in \
                 Settings.",
                b.obs_version()
            ));
        }
        lines.push(
            tr!(
                "Збере OBS і плагіни з нуля: кеш збірки OBS очищується, застарілі блокування git після обірваного \
                 збирання прибираються. Потім — ті самі перевірки NVENC і бібліотек і встановлення через pacman.",
                "Builds OBS and plugins from scratch: the OBS build cache is cleared, stale git locks from a cut-off \
                 build are removed. Then the same NVENC and library checks and installation with pacman."
            )
            .into(),
        );
        lines.push(String::new());
        lines.push(
            tr!(
                "Коли потрібно: збирання обірвалося (вимкнулося світло, закрилося вікно), OBS поводиться дивно чи \
                 падає. Для звичайного оновлення досить «Оновити».",
                "When needed: a build was cut off (power outage, closed window), or OBS behaves oddly or crashes. \
                 For a regular update, “Update” is enough."
            )
            .into(),
        );
        lines.push(
            tr!(
                "Довше, ніж оновлення: 30–90 хв, бо все компілюється з нуля.",
                "Takes longer than an update: 30–90 min, as everything is compiled from scratch."
            )
            .into(),
        );
        self.dialog = Some(Dialog {
            title: tr!("Перезібрати OBS начисто?", "Rebuild OBS from scratch?").into(),
            lines,
            confirm: tr!("Перезібрати", "Rebuild"),
            danger: false,
            alt: None,
            action: DialogAction::Build(BuildMode::Rebuild),
        });
    }

    /// Діалог «Сповіщення про оновлення»: що це, чи ввімкнено, увімкнути чи вимкнути.
    pub(super) fn notifier_dialog(&mut self) {
        let st = service::status();
        let mut lines = vec![
            tr!(
                "Раз на 12 годин OBS-Legacy у фоні перевіряє, чи вийшли нові версії OBS, плагінів і самої \
                 програми, а після оновлень системи — чи власному OBS не бракує бібліотек. Про кожну новину — одне \
                 сповіщення на робочому столі; клацання по ньому відкриває OBS-Legacy.",
                "Every 12 hours OBS-Legacy checks in the background for new versions of OBS, plugins and the program \
                 itself, and after system updates — whether your OBS is missing libraries. Each piece of news is one \
                 desktop notification; clicking it opens OBS-Legacy."
            )
            .to_string(),
            String::new(),
        ];
        let (title, confirm, enable) = match st.installed.first() {
            Some(p) => {
                let state = if st.running.is_some() {
                    tr!("працює", "running")
                } else {
                    tr!("зараз не працює", "not running now")
                };
                let (backend, shown) = (p.backend.label(), p.shown());
                lines.push(trf!("Увімкнено: {backend} ({shown}), {state}.", "On: {backend} ({shown}), {state}."));
                lines.extend(st.last_line().map(|l| format!("{l}.")));
                (tr!("Сповіщення про оновлення", "Update notifications"), tr!("Вимкнути", "Turn off"), false)
            }
            None => {
                if let Some(best) = service::available().first() {
                    lines.push(trf!(
                        "Перевірку запускатиме {} — це підходить для твоєї системи. OBS-Legacy можна закривати.",
                        "The check will run via the {} — that suits your system. You can close OBS-Legacy.",
                        best.backend.label()
                    ));
                }
                lines.push(
                    tr!(
                        "Мережа потрібна лише для коротких запитів до GitHub (git ls-remote).",
                        "The network is needed only for short requests to GitHub (git ls-remote)."
                    )
                    .into(),
                );
                (
                    tr!("Увімкнути сповіщення про оновлення?", "Turn on update notifications?"),
                    tr!("Увімкнути", "Turn on"),
                    true,
                )
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

    /// «Встановити пакети»: які саме й що спитає скрипт.
    pub(super) fn deps_dialog(&mut self) {
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

    /// «Видалити OBS-Legacy»: що приберуть, а що лишиться.
    pub(super) fn uninstall_dialog(&mut self) {
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

    /// «Оновити OBS-Legacy»: з вихідних кодів (git pull і збирання) чи готова програма з релізу.
    pub(super) fn self_update_dialog(&mut self) {
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
}
