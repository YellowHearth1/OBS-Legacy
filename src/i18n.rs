//! Мова інтерфейсу: українська або англійська.
//!
//! `UI_LANG` у settings.env: `auto` (типово) — за локаллю системи (`LC_ALL`, `LC_MESSAGES`, `LANG`, а якщо їх
//! немає, як у служби користувача, — `/etc/locale.conf`): `uk…` — українська, будь-яка інша — англійська;
//! `uk` чи `en` — примусово. Ту саму мову TUI передає скрипту (`OBS_LEGACY_LANG`).
//!
//! Тексти пишуться парами просто в коді: `tr!("Українською", "In English")`, а з підстановками —
//! `trf!("Вийшов OBS {v}", "OBS {v} is out")`.

use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Uk,
    En,
}

impl Lang {
    /// Код для скрипта й налаштувань: `uk` чи `en`.
    pub fn code(self) -> &'static str {
        match self {
            Lang::Uk => "uk",
            Lang::En => "en",
        }
    }
}

static EN: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
thread_local! {
    /// Мова лише для цього потоку: тести йдуть паралельно й не заважають одне одному.
    static TEST_LANG: std::cell::Cell<Option<Lang>> = const { std::cell::Cell::new(None) };
}

pub fn set(lang: Lang) {
    EN.store(lang == Lang::En, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    #[cfg(test)]
    if let Some(l) = TEST_LANG.with(std::cell::Cell::get) {
        return l;
    }
    if EN.load(Ordering::Relaxed) { Lang::En } else { Lang::Uk }
}

pub fn en() -> bool {
    lang() == Lang::En
}

/// Варіант для поточної мови.
pub fn pick<T>(uk: T, en: T) -> T {
    if self::en() { en } else { uk }
}

/// Мова з налаштування (`auto`, `uk`, `en`) і локалі системи.
pub fn resolve(setting: &str, locale: Option<&str>) -> Lang {
    match setting.trim() {
        "uk" => Lang::Uk,
        "en" => Lang::En,
        _ if locale.is_some_and(|l| l.starts_with("uk")) => Lang::Uk,
        _ => Lang::En,
    }
}

/// Локаль системи: змінні середовища, а без них (служба користувача) — /etc/locale.conf.
pub fn system_locale() -> Option<String> {
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var)
            && !v.is_empty()
        {
            return Some(v);
        }
    }
    let text = std::fs::read_to_string("/etc/locale.conf").ok()?;
    locale_conf_lang(&text)
}

fn locale_conf_lang(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("LANG="))
        .map(|v| v.trim().trim_matches(['"', '\'']).to_string())
        .filter(|v| !v.is_empty())
}

/// Виконати `f` з мовою `lang` у цьому потоці (лише в тестах).
#[cfg(test)]
pub fn with<R>(lang: Lang, f: impl FnOnce() -> R) -> R {
    let old = TEST_LANG.with(|c| c.replace(Some(lang)));
    let r = f();
    TEST_LANG.with(|c| c.set(old));
    r
}

/// Текст поточною мовою: `tr!("українською", "in English")`.
#[macro_export]
macro_rules! tr {
    ($uk:expr, $en:expr $(,)?) => {
        $crate::i18n::pick($uk, $en)
    };
}

/// `format!` поточною мовою: `trf!("Вийшов OBS {v}", "OBS {v} is out")`.
#[macro_export]
macro_rules! trf {
    ($uk:literal, $en:literal $(, $($arg:tt)*)?) => {
        if $crate::i18n::en() { format!($en $(, $($arg)*)?) } else { format!($uk $(, $($arg)*)?) }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_choice() {
        assert_eq!(resolve("auto", Some("uk_UA.UTF-8")), Lang::Uk);
        assert_eq!(resolve("", Some("en_US.UTF-8")), Lang::En);
        assert_eq!(resolve("auto", Some("C")), Lang::En);
        assert_eq!(resolve("auto", None), Lang::En);
        assert_eq!(resolve("uk", Some("de_DE.UTF-8")), Lang::Uk);
        assert_eq!(resolve("en", Some("uk_UA.UTF-8")), Lang::En);
        assert_eq!(locale_conf_lang("# x\nLANG=\"uk_UA.UTF-8\"\nLC_TIME=C\n").as_deref(), Some("uk_UA.UTF-8"));
        assert_eq!(locale_conf_lang("LC_TIME=C\n"), None);
    }

    #[test]
    fn texts_follow_the_language() {
        let v = "33.0.1";
        with(Lang::Uk, || {
            assert_eq!(tr!("Так", "Yes"), "Так");
            assert_eq!(trf!("Вийшов OBS {v}", "OBS {v} is out"), "Вийшов OBS 33.0.1");
        });
        with(Lang::En, || {
            assert_eq!(tr!("Так", "Yes"), "Yes");
            assert_eq!(trf!("Вийшов OBS {}", "OBS {} is out", v), "OBS 33.0.1 is out");
        });
    }
}
