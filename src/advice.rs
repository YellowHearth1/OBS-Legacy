//! «Що далі»: одна зрозуміла порада на головному екрані. Залежить від
//! системи, встановлених збірок і останніх звітів, тож новачкові не треба
//! розбиратися в меню — досить натиснути Enter.

use std::path::PathBuf;

use crate::app::Action;
use crate::reports::{Outcome, ReportInfo};
use crate::system::SysInfo;
use crate::updates::{self, Facts};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Info,
    Warn,
    Err,
}

/// Де людина на шляху «перевірити → встановити → користуватися».
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Check,
    Install,
    Use,
}

#[derive(Clone, Debug)]
pub struct Advice {
    pub tone: Tone,
    pub stage: Stage,
    pub title: String,
    pub detail: Option<String>,
    /// Що робити — по пункту на рядок.
    pub todo: Vec<String>,
    /// Що запускає Enter.
    pub action: Option<Action>,
    /// Друга кнопка: інший розумний шлях (наприклад, одразу встановлювати).
    pub alt: Option<Action>,
    /// Звіт, який відкриває «o».
    pub report: Option<PathBuf>,
}

impl Advice {
    fn new(tone: Tone, stage: Stage, title: impl Into<String>) -> Self {
        Self { tone, stage, title: title.into(), detail: None, todo: Vec::new(), action: None, alt: None, report: None }
    }

    fn detail(mut self, text: impl Into<String>) -> Self {
        self.detail = Some(text.into());
        self
    }

    fn todo(mut self, text: impl Into<String>) -> Self {
        self.todo.push(text.into());
        self
    }

    fn action(mut self, action: Action) -> Self {
        self.action = Some(action);
        self
    }

    fn alt(mut self, action: Action) -> Self {
        self.alt = Some(action);
        self
    }

    fn report(mut self, r: &ReportInfo) -> Self {
        self.report = Some(r.path.clone());
        if let Some(p) = &r.problem {
            if self.detail.is_none() {
                self.detail = Some(p.text.clone());
            }
            self.todo.extend(p.hints.iter().cloned());
        }
        self
    }
}

pub struct Input<'a> {
    pub sys: &'a SysInfo,
    /// Нові версії (None — ще не перевіряли).
    pub facts: Option<&'a Facts>,
    /// Від найновішого.
    pub reports: &'a [ReportInfo],
    pub selected_plugins: &'a [String],
    /// Пакети для збірки, яких бракує (None — ще невідомо).
    pub missing_packages: Option<&'a [String]>,
    /// Чи можна оновити OBS-Legacy з меню.
    pub self_update: bool,
    /// Чи ввімкнено сповіщення про оновлення (None — ще невідомо).
    pub notifier: Option<bool>,
    /// Чи працює зараз збирання (тоді звіт без «Результат:» — не обрив, а поточний запуск).
    pub build_running: bool,
}

/// Чи пропонував звіт поставити пакети («[ВИПРАВИТИ] deps»).
fn wants_deps(r: &ReportInfo) -> bool {
    r.problem.as_ref().and_then(|p| p.fix.as_ref()).is_some_and(|f| f.command == "deps")
}

fn missing_packages_advice(stage: Stage, missing: &[String]) -> Advice {
    let n = missing.len();
    Advice::new(Tone::Warn, stage, format!("Бракує пакетів для збірки: {n}"))
        .detail(format!(
            "{}. Програма поставить лише їх через pacman (залежності pacman додасть сам), без оновлення \
             системи. Потім перевір систему ще раз — усе має бути ✓.",
            missing.join(", ")
        ))
        .action(Action::Deps)
        .alt(Action::Install)
}

/// Мітка часу збірки з її назви «20261028-174550_33.0.0».
fn build_stamp(label: &str) -> &str {
    label.split('_').next().unwrap_or("")
}

pub fn advise(i: &Input) -> Advice {
    let s = i.sys;
    let active = s.active_build();
    let last = |cmds: &[&str]| i.reports.iter().find(|r| cmds.contains(&r.command.as_str()));
    let last_check = last(&["check"]);
    let last_install = last(&["install", "update", "rebuild"]);
    let stage = if active.is_some() {
        Stage::Use
    } else if last_check.is_some_and(|r| r.outcome == Outcome::Success) {
        Stage::Install
    } else {
        Stage::Check
    };

    if s.is_root {
        return Advice::new(Tone::Err, stage, "OBS-Legacy запущено від root")
            .detail("Збирати OBS від root небезпечно, тому скрипт відмовиться працювати.")
            .todo(
                "Закрий програму і запусти obs-legacy від свого користувача, без sudo — пароль вона попросить сама.",
            );
    }
    if !s.has_pacman {
        return Advice::new(Tone::Err, stage, "Потрібен Artix або Arch Linux")
            .detail("OBS-Legacy ставить пакети через pacman, а його в цій системі немає.");
    }
    if s.gpus.is_empty() && s.driver.is_none() {
        return Advice::new(Tone::Err, stage, "Не бачу відеокарти NVIDIA")
            .detail("Драйвер NVIDIA не встановлено або його модуль не завантажився.")
            .todo(
                "Для GTX 900/1000 встанови драйвер гілки 580xx (nvidia-580xx-dkms і nvidia-580xx-utils з AUR) \
                 і перезавантаж комп'ютер.",
            )
            .action(Action::Check);
    }
    if s.nvenc_compatible() == Some(false) {
        let need = s.headers_api.clone().unwrap_or_default();
        return Advice::new(Tone::Err, stage, format!("Драйвер не підтримує NVENC API {need}"))
            .detail(format!("Драйвер знає NVENC API лише до {}.", s.nvenc_api.clone().unwrap_or_default()))
            .todo("Онови драйвер до гілки 580xx і перезавантаж комп'ютер.");
    }

    // Збирання обірвалося: звіт без «Результат:», а скрипт уже не працює (вимкнулося світло,
    // закрилося вікно). Лише якщо це новіше за встановлену збірку — інакше давня історія.
    if !i.build_running
        && let Some(r) = last_install.filter(|r| r.outcome == Outcome::Unknown)
        && active.is_none_or(|b| r.stamp.as_str() > build_stamp(&b.label))
    {
        let now = active.map_or_else(
            || "Нічого не встановлено й не змінено.".to_string(),
            |b| format!("Працює попередній OBS {} — його не зачеплено.", b.obs_version()),
        );
        let mut a = Advice::new(Tone::Warn, stage, "Минуле збирання обірвалося")
            .detail(format!(
                "Мабуть, вимкнулося світло чи закрилося вікно програми. {now} Перезбери начисто — це надійніше, \
                 ніж продовжувати з половини."
            ))
            .action(Action::Rebuild)
            .alt(Action::Install);
        a.report = Some(r.path.clone());
        return a;
    }

    if let Some(b) = active {
        let version = b.obs_version().to_string();
        let failed = b.plugins_failed();
        if let Some(r) =
            last_install.filter(|r| r.outcome == Outcome::Failed && r.stamp.as_str() > build_stamp(&b.label))
        {
            return Advice::new(Tone::Warn, stage, format!("Оновлення не вдалося — працює OBS {version}"))
                .report(r)
                .action(Action::Install)
                .alt(Action::Rebuild);
        }
        // Набір зібрали, але встановлення скасували на запитанні: «Оновити» поставить його
        // без перезбирання (у скрипті — pending_set).
        if last_install.is_some_and(|r| r.outcome == Outcome::Cancelled && r.stamp.as_str() > build_stamp(&b.label))
            && let Some(ready) = s
                .builds
                .iter()
                .filter(|x| x.complete && !x.legacy && x.label > b.label)
                .max_by(|x, y| x.label.cmp(&y.label))
        {
            return Advice::new(Tone::Warn, stage, "Пакети зібрано, але не встановлено")
                .detail(format!(
                    "Набір {} готовий: встановлення скасували на запитанні. «Встановити / оновити OBS» поставить \
                     його без перезбирання — на запитання відповідай y латиницею.",
                    ready.label
                ))
                .action(Action::Install);
        }
        if let Some(cpu) = s.built_for_other_cpu() {
            return Advice::new(Tone::Warn, stage, "OBS зібрано під інший процесор")
                .detail(format!(
                    "Встановлені пакети оптимізовано під {cpu}. На цьому процесорі OBS може аварійно \
                     завершуватися — перезбери його під цей комп'ютер."
                ))
                .action(Action::Install);
        }
        if !failed.is_empty() {
            return Advice::new(Tone::Warn, stage, format!("Не зібралися плагіни: {}", failed.join(", ")))
                .detail("OBS працює без них, а попередня збірка з ними доступна в «Відкат на іншу збірку».")
                .todo("Спробуй «Встановити / оновити OBS» пізніше: автор плагіна, можливо, ще не підтримав цю версію OBS.")
                .action(Action::Install);
        }
        let facts = i.facts;
        if let Some(f) = facts
            && let Some(latest) = f.obs_update()
        {
            let plugins = if f.plugin_updates.is_empty() {
                String::new()
            } else {
                format!(" і свіжі плагіни ({})", updates::plugin_list(&f.plugin_updates))
            };
            return Advice::new(Tone::Info, stage, format!("Вийшов OBS {latest}"))
                .detail(format!(
                    "У тебе OBS {version}. Оновлення збере нову версію з NVENC{plugins}, а поточна лишиться для відкату."
                ))
                .action(Action::Install);
        }
        if let Some(f) = facts.filter(|f| !f.plugin_updates.is_empty()) {
            return Advice::new(Tone::Info, stage, "Вийшли нові версії плагінів")
                .detail(format!(
                    "{}. Плагіни оновлюються разом з OBS: «Встановити / оновити OBS» збере їх, а поточна збірка \
                     лишиться для відкату.",
                    updates::plugin_list(&f.plugin_updates)
                ))
                .action(Action::Install);
        }
        if b.legacy {
            return Advice::new(Tone::Info, stage, "OBS-Legacy тепер ставиться пакетами pacman")
                .detail(
                    "«Встановити / оновити OBS» збере obs-studio-legacy-580xx і плагіни під твій процесор і драйвер та \
                     поставить їх у /usr, як OBS з репозиторію: сторонні плагіни ставитимуться на нього як завжди. \
                     pacman спитає, чи замінити obs-studio-git, а стара збірка з /opt прибереться сама.",
                )
                .action(Action::Install);
        }
        if let Some(f) = facts
            && let Some(new) = f.ffmpeg_new.as_deref()
        {
            return Advice::new(Tone::Info, stage, format!("У репозиторії новий ffmpeg {new}"))
                .detail(format!(
                    "Твій ffmpeg-legacy-580xx зібрано з {}. «Встановити / оновити OBS» перезбере його з NVENC під \
                     твій драйвер і процесор (ffmpeg — ~15 хв); до того працює поточний.",
                    f.ffmpeg_built.as_deref().unwrap_or("старішої версії")
                ))
                .action(Action::Install);
        }
        let installed: Vec<String> = b.plugins().into_iter().map(|(id, _)| id).collect();
        if i.selected_plugins.iter().any(|p| !installed.contains(p)) {
            return Advice::new(Tone::Info, stage, "Нові плагіни ще не встановлено")
                .detail("Обрані плагіни з'являться після «Встановити / оновити OBS».")
                .action(Action::Install);
        }
        if i.self_update
            && let Some(f) = facts
            && let Some(latest) = f.self_update()
        {
            return Advice::new(Tone::Info, stage, format!("Вийшов OBS-Legacy {}", latest.trim_start_matches('v')))
                .detail(format!(
                    "У тебе {}. Оновлення змінить лише саму програму — твій OBS лишиться як є.",
                    f.self_version
                ))
                .action(Action::SelfUpdate);
        }
        let mut detail =
            format!("OBS {version} з NVENC встановлено. Запускай його як завжди: з меню програм або командою obs.");
        let mut a = Advice::new(Tone::Ok, stage, "Усе готово — можна стрімити").action(Action::Launch);
        if i.notifier == Some(false) {
            detail.push_str(" Щоб не пропустити нові версії OBS і плагінів, увімкни сповіщення про оновлення.");
            a = a.alt(Action::Notify);
        }
        return a.detail(detail);
    }

    if s.driver_major().is_some_and(|m| m > 580) {
        return Advice::new(Tone::Info, stage, "Тобі, мабуть, вистачить звичайного OBS")
            .detail(
                "Драйвер новіший за 580xx, тож NVENC працює і в OBS з репозиторію. OBS-Legacy потрібен \
                 насамперед для старих відеокарт (GTX 900/1000) на драйвері 580xx.",
            )
            .action(Action::Check);
    }
    let install_is_newer = match (last_install, last_check) {
        (Some(inst), Some(chk)) => inst.stamp > chk.stamp,
        (Some(_), None) => true,
        _ => false,
    };
    if let Some(r) = last_install.filter(|r| install_is_newer && r.outcome == Outcome::Failed) {
        return Advice::new(Tone::Err, stage, "Встановлення не вдалося")
            .report(r)
            .action(Action::Install)
            .alt(Action::Rebuild);
    }
    let last_deps = last(&["deps"]);
    if let Some(r) = last_deps.filter(|r| r.outcome == Outcome::Failed && last_check.is_none_or(|c| r.stamp > c.stamp))
    {
        return Advice::new(Tone::Err, stage, "Не вдалося встановити пакети").report(r).action(Action::Deps);
    }
    let missing = i.missing_packages;
    match last_check {
        Some(r) if r.outcome == Outcome::Failed => {
            // Скрипт міг підказати, що виправити (поставити пакети, звільнити місце).
            let fix = r.problem.as_ref().and_then(|p| p.fix.as_ref()).and_then(|f| crate::app::fix_action(&f.command));
            let fix = match fix {
                Some(Action::Deps) if missing.is_some_and(<[String]>::is_empty) => Action::Check,
                Some(a) => a,
                None => Action::Check,
            };
            Advice::new(Tone::Err, stage, "Перевірка знайшла проблему").report(r).action(fix)
        }
        Some(r) if r.outcome == Outcome::Success => match missing {
            Some(m) if !m.is_empty() => missing_packages_advice(stage, m),
            // Звіт ще каже «бракує пакетів», а їх уже поставили — перевірити ще раз.
            Some(_) if wants_deps(r) => Advice::new(Tone::Info, stage, "Пакети на місці — перевір систему ще раз")
                .detail("Тепер пройде й пробна конфігурація CMake, і всі етапи будуть ✓.")
                .action(Action::Check)
                .alt(Action::Install),
            None if wants_deps(r) => Advice::new(Tone::Warn, stage, "Бракує пакетів для збірки")
                .report(r)
                .action(Action::Deps)
                .alt(Action::Install),
            _ => {
                let mut a = Advice::new(Tone::Ok, stage, "Систему перевірено — можна встановлювати").report(r);
                if a.detail.is_none() {
                    a = a.detail("Збирання триває 30–90 хв. Пароль sudo/doas знадобиться під час встановлення.");
                }
                a.action(Action::Install)
            }
        },
        Some(r) => Advice::new(Tone::Info, stage, "Перевірку не завершено")
            .detail("Запусти її ще раз — уже завантажене не завантажуватиметься знову.")
            .report(r)
            .action(Action::Check),
        None => Advice::new(Tone::Info, stage, "Почни з перевірки системи")
            .detail(
                "Вона нічого не змінює і триває 5–15 хв: перевірить драйвер і пакети та завантажить \
                 вихідні коди OBS.",
            )
            .action(Action::Check),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::{Level, Problem};
    use crate::system::BuildInfo;

    fn sys() -> SysInfo {
        SysInfo {
            gpus: vec!["NVIDIA GeForce GTX 1060 3GB".into()],
            driver: Some("580.196.07".into()),
            nvenc_api: Some("13.0".into()),
            headers_api: Some("13.0".into()),
            has_pacman: true,
            ..Default::default()
        }
    }

    fn report(stamp: &str, command: &str, outcome: Outcome, hint: Option<&str>) -> ReportInfo {
        ReportInfo {
            path: PathBuf::from(format!("/r/{stamp}-{command}.txt")),
            stamp: stamp.into(),
            command: command.into(),
            outcome,
            warnings: 0,
            problem: hint.map(|h| Problem {
                level: Level::Error,
                text: "щось".into(),
                hints: vec![h.into()],
                fix: None,
            }),
        }
    }

    fn base<'a>(s: &'a SysInfo, reports: &'a [ReportInfo]) -> Input<'a> {
        Input {
            sys: s,
            facts: None,
            reports,
            selected_plugins: &[],
            missing_packages: None,
            self_update: false,
            notifier: None,
            build_running: false,
        }
    }

    fn advise_with(s: &SysInfo, reports: &[ReportInfo]) -> Advice {
        advise(&base(s, reports))
    }

    #[test]
    fn beginner_path() {
        let s = sys();
        let a = advise_with(&s, &[]);
        assert_eq!((a.stage, a.action), (Stage::Check, Some(Action::Check)));

        let failed = [report("20261028-173417", "check", Outcome::Failed, Some("Видали кеш."))];
        let a = advise_with(&s, &failed);
        assert_eq!(a.tone, Tone::Err);
        assert_eq!(a.todo, ["Видали кеш."]);
        assert!(a.report.is_some());

        let passed = [report("20261028-174550", "check", Outcome::Success, None), failed[0].clone()];
        let a = advise_with(&s, &passed);
        assert_eq!((a.stage, a.action, a.tone), (Stage::Install, Some(Action::Install), Tone::Ok));
    }

    #[test]
    fn cancelled_install_leaves_ready_set() {
        let mut s = sys();
        let mut b =
            BuildInfo { label: "20261030-110840_33.0.1".into(), active: true, complete: true, ..Default::default() };
        b.fields.insert("obs_version".into(), "33.0.1".into());
        s.builds.push(b);
        s.builds.push(BuildInfo { label: "20261030-122521_33.0.1".into(), complete: true, ..Default::default() });
        let cancelled = [report("20261030-122515", "rebuild", Outcome::Cancelled, None)];
        let a = advise_with(&s, &cancelled);
        assert_eq!((a.tone, a.action), (Tone::Warn, Some(Action::Install)));
        assert_eq!(a.title, "Пакети зібрано, але не встановлено");
        assert!(a.detail.unwrap().contains("20261030-122521_33.0.1"));
        // Неповний набір не пропонуємо.
        s.builds[1].complete = false;
        assert_eq!(advise_with(&s, &cancelled).action, Some(Action::Launch));
        s.builds[1].complete = true;
        // Набір уже встановлено — звичайна порада.
        s.builds[0].active = false;
        s.builds[1].active = true;
        s.builds[1].fields.insert("obs_version".into(), "33.0.1".into());
        assert_eq!(advise_with(&s, &cancelled).action, Some(Action::Launch));
    }

    #[test]
    fn installed_build() {
        let mut s = sys();
        let mut b =
            BuildInfo { label: "20261028-190000_33.0.0".into(), active: true, complete: true, ..Default::default() };
        b.fields.insert("obs_version".into(), "33.0.0".into());
        s.builds.push(b);
        let a = advise_with(&s, &[]);
        assert_eq!((a.stage, a.action, a.tone), (Stage::Use, Some(Action::Launch), Tone::Ok));

        // Стара схема (/opt) — порада поставити пакети.
        s.builds[0].legacy = true;
        let a = advise_with(&s, &[]);
        assert_eq!((a.action, a.title.as_str()), (Some(Action::Install), "OBS-Legacy тепер ставиться пакетами pacman"));
        s.builds[0].legacy = false;
        // Збірку під native перенесли на інший процесор — перезібрати.
        s.builds[0].fields.insert("cpu_tuning".into(), "native".into());
        s.builds[0].fields.insert("cpu_model".into(), "Intel Xeon E3-1275 V2".into());
        s.cpu_model = "AMD Ryzen 5 5600".into();
        let a = advise_with(&s, &[]);
        assert_eq!((a.tone, a.action), (Tone::Warn, Some(Action::Install)));
        s.cpu_model = "Intel Xeon E3-1275 V2".into();
        assert_eq!(advise_with(&s, &[]).action, Some(Action::Launch));

        // Нова спроба оновлення впала — стара збірка працює.
        let failed = [report("20261029-100000", "install", Outcome::Failed, Some("Повтори."))];
        let a = advise_with(&s, &failed);
        assert_eq!(a.tone, Tone::Warn);
        // Стара невдача (до активної збірки) нічого не означає.
        let old = [report("20261027-100000", "install", Outcome::Failed, None)];
        assert_eq!(advise_with(&s, &old).tone, Tone::Ok);

        let f = Facts {
            obs_installed: Some("33.0.0".into()),
            obs_latest: Some("33.0.1".into()),
            self_version: "0.0.1".into(),
            ..Default::default()
        };
        let a = advise(&Input { facts: Some(&f), ..base(&s, &[]) });
        assert_eq!(a.action, Some(Action::Install));
        assert!(a.title.contains("33.0.1"));
        // Зафіксована версія OBS — про нову не кажемо.
        let pinned = Facts { obs_pinned: Some("33.0.0".into()), ..f.clone() };
        assert_eq!(advise(&Input { facts: Some(&pinned), ..base(&s, &[]) }).action, Some(Action::Launch));

        // Нові плагіни — те саме оновлення.
        let blur = updates::PluginUpdate {
            id: "composite-blur".into(),
            name: "Composite Blur".into(),
            installed: "v1.6.1".into(),
            latest: "v1.6.2".into(),
        };
        let plugins = Facts { obs_latest: Some("33.0.0".into()), plugin_updates: vec![blur], ..f.clone() };
        let a = advise(&Input { facts: Some(&plugins), ..base(&s, &[]) });
        assert_eq!((a.action, a.title.as_str()), (Some(Action::Install), "Вийшли нові версії плагінів"));
        assert!(a.detail.unwrap().starts_with("Composite Blur v1.6.2."));

        // Новий ffmpeg у репозиторії — перезібрати ffmpeg-legacy-580xx.
        let ff = Facts {
            obs_latest: Some("33.0.0".into()),
            ffmpeg_built: Some("2:9.0.2-2".into()),
            ffmpeg_new: Some("2:9.0.3-1".into()),
            ..f.clone()
        };
        let a = advise(&Input { facts: Some(&ff), ..base(&s, &[]) });
        assert_eq!((a.action, a.title.as_str()), (Some(Action::Install), "У репозиторії новий ffmpeg 2:9.0.3-1"));

        // Новий OBS-Legacy — лише коли його можна оновити з меню.
        let me = Facts { obs_latest: Some("33.0.0".into()), self_latest: Some("v0.0.2".into()), ..f };
        assert_eq!(advise(&Input { facts: Some(&me), ..base(&s, &[]) }).action, Some(Action::Launch));
        let a = advise(&Input { facts: Some(&me), self_update: true, ..base(&s, &[]) });
        assert_eq!((a.action, a.title.as_str()), (Some(Action::SelfUpdate), "Вийшов OBS-Legacy 0.0.2"));

        // Усе готово, а сповіщення вимкнено — друга кнопка їх вмикає.
        let a = advise(&Input { notifier: Some(false), ..base(&s, &[]) });
        assert_eq!((a.action, a.alt), (Some(Action::Launch), Some(Action::Notify)));
        assert_eq!(advise(&Input { notifier: Some(true), ..base(&s, &[]) }).alt, None);
    }

    #[test]
    fn missing_packages_offer_a_fix() {
        let s = sys();
        let mut r = report("20261028-224849", "check", Outcome::Success, Some("Постав їх."));
        if let Some(p) = r.problem.as_mut() {
            p.fix = Some(crate::analyzer::Fix {
                command: "deps".into(), label: "Встановити пакети".into()
            });
        }
        let reports = [r];
        let missing = ["swig".to_string(), "asio".to_string()];
        let with = |m: Option<&[String]>| advise(&Input { missing_packages: m, ..base(&s, &reports) });
        let a = with(Some(&missing));
        assert_eq!((a.tone, a.action, a.alt), (Tone::Warn, Some(Action::Deps), Some(Action::Install)));
        assert!(a.detail.unwrap().contains("swig, asio"));
        // Пакети поставили вручну — перевірити ще раз.
        let a = with(Some(&[]));
        assert_eq!((a.tone, a.action), (Tone::Info, Some(Action::Check)));
        // Поки невідомо — віримо звіту.
        assert_eq!(with(None).action, Some(Action::Deps));
        // Повна перевірка без зауважень — зелений шлях.
        let clean = [report("20261028-230206", "check", Outcome::Success, None)];
        let a = advise(&Input { missing_packages: Some(&[]), ..base(&s, &clean) });
        assert_eq!((a.tone, a.action), (Tone::Ok, Some(Action::Install)));
    }

    #[test]
    fn interrupted_build_offers_rebuild() {
        let mut s = sys();
        let mut b =
            BuildInfo { label: "20261030-110840_33.0.1".into(), active: true, complete: true, ..Default::default() };
        b.fields.insert("obs_version".into(), "33.0.1".into());
        s.builds.push(b);
        // Звіт оновлення без «Результат:» — світло вимкнулося посеред збирання.
        let cut = [report("20261030-120000", "update", Outcome::Unknown, None)];
        let a = advise_with(&s, &cut);
        assert_eq!((a.tone, a.action, a.alt), (Tone::Warn, Some(Action::Rebuild), Some(Action::Install)));
        assert!(a.title.contains("обірвалося"));
        assert!(a.detail.unwrap().contains("33.0.1"));
        assert!(a.report.is_some());
        // Поки збирання ще йде, це не обрив.
        assert_eq!(advise(&Input { build_running: true, ..base(&s, &cut) }).action, Some(Action::Launch));
        // Давній обрив (до встановленої збірки) нічого не означає.
        let old = [report("20261029-100000", "rebuild", Outcome::Unknown, None)];
        assert_eq!(advise_with(&s, &old).action, Some(Action::Launch));
        // Невдале оновлення — друга кнопка «Перезібрати».
        let failed = [report("20261030-120000", "update", Outcome::Failed, Some("Повтори."))];
        assert_eq!(advise_with(&s, &failed).alt, Some(Action::Rebuild));
    }

    #[test]
    fn missing_driver_is_an_error() {
        let s = SysInfo { has_pacman: true, ..Default::default() };
        assert_eq!(advise_with(&s, &[]).tone, Tone::Err);
    }
}
