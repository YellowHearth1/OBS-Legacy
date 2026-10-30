//! Файл налаштувань ~/.config/obs-legacy/settings.env (рядки KEY=VALUE).
//! Його ж читає скрипт збирання, тож TUI і запуск скрипта вручну
//! поводяться однаково. Невідомі рядки й коментарі зберігаються як є.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::script;

pub const DEFAULT_PLUGINS: &str = "obs-multi-rtmp";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    /// Додатне ціле; `optional` — порожнє значення означає «авто».
    Number {
        optional: bool,
    },
    Toggle,
    /// Один з варіантів (значення, пояснення); Enter перебирає їх по колу.
    Choice(&'static [(&'static str, &'static str)]),
}

impl Kind {
    /// Пояснення до значення варіанта («native» → «цей процесор»).
    pub fn choice_label(self, value: &str) -> Option<&'static str> {
        match self {
            Kind::Choice(options) => options.iter().find(|(v, _)| *v == value).map(|(_, l)| *l),
            _ => None,
        }
    }

    /// Наступний варіант після `value` (по колу).
    pub fn next_choice(self, value: &str) -> Option<&'static str> {
        let Kind::Choice(options) = self else { return None };
        let i = options.iter().position(|(v, _)| *v == value).map_or(0, |i| (i + 1) % options.len());
        options.get(i).map(|(v, _)| *v)
    }
}

pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub kind: Kind,
    pub default: &'static str,
    /// Для досвідчених: показується окремою групою нижче.
    pub advanced: bool,
}

/// Спершу основне, потім — для досвідчених (так їх і показує екран).
pub const FIELDS: &[Field] = &[
    Field {
        key: "OBS_REF",
        label: "Версія OBS",
        help: "stable — найновіший стабільний реліз (рекомендовано). Можна вказати тег (33.0.0), гілку (master) \
               або коміт. Якщо нова версія OBS колись перестане працювати з драйвером 580xx — зафіксуй тут останню робочу.",
        kind: Kind::Text,
        default: "stable",
        advanced: false,
    },
    Field {
        key: "CPU_TUNING",
        label: "Під який процесор",
        help: "native — саме під цей процесор: найшвидше, програма сама визначає його можливості (AVX тощо). Такі \
               пакети працюють лише на цьому процесорі — після його заміни перезбери OBS. generic — переносно, як \
               пакети з репозиторію. x86-64-v2/v3/v4 — рівень набору інструкцій, якщо пакети потрібні для кількох \
               комп'ютерів. Решта прапорців — з makepkg.conf твого дистрибутива.",
        kind: Kind::Choice(&[
            ("native", "саме під цей процесор"),
            ("generic", "переносно, як у репозиторії"),
            ("x86-64-v2", "рівень v2 (SSE4.2)"),
            ("x86-64-v3", "рівень v3 (AVX2)"),
            ("x86-64-v4", "рівень v4 (AVX-512)"),
        ]),
        default: "native",
        advanced: false,
    },
    Field {
        key: "BUILD_FFMPEG",
        label: "Збирати ffmpeg",
        help: "Збирати ffmpeg-legacy-580xx: той самий ffmpeg, що в репозиторії, але з NVENC для драйвера 580xx і під \
               цей процесор. Він замінить системний ffmpeg (pacman спитає) — NVENC запрацює і в «Власний вивід \
               (FFmpeg)» в OBS, і в Kdenlive, і в ffmpeg у терміналі. Перезбирається лише тоді, коли в репозиторії \
               виходить новий ffmpeg. Сам OBS кодує NVENC власним модулем і без цього.",
        kind: Kind::Toggle,
        default: "1",
        advanced: false,
    },
    Field {
        key: "KEEP_VERSIONS",
        label: "Скільки наборів тримати",
        help: "Скільки наборів пакетів (OBS + плагіни, ~0,3–0,6 ГБ кожен) зберігати разом зі встановленим — для \
               відкату через pacman без перезбирання.",
        kind: Kind::Number { optional: false },
        default: "2",
        advanced: false,
    },
    Field {
        key: "INSTALL_PACMAN_HOOK",
        label: "Перевірка після оновлень",
        help: "Пакет OBS містить pacman-хук: після кожного оновлення системи він перевіряє, чи пакетам OBS-Legacy \
               вистачає бібліотек і чи не вийшов новий ffmpeg, і підказує, коли перезібрати. Нічого не змінює. \
               Діє з наступного встановлення.",
        kind: Kind::Toggle,
        default: "1",
        advanced: false,
    },
    Field {
        key: "UI_BACKGROUND",
        label: "Суцільний фон",
        help: "Темне полотно під усім інтерфейсом, щоб текст добре читався в прозорих терміналах \
               (Kitty, Alacritty, Konsole). Вимкни, якщо хочеш бачити тло термінала.",
        kind: Kind::Toggle,
        default: "1",
        advanced: false,
    },
    Field {
        key: "BUILD_JOBS",
        label: "Потоки компіляції",
        help: "Порожньо — автоматично (з урахуванням вільної пам'яті: ~2 ГБ на потік).",
        kind: Kind::Number { optional: true },
        default: "",
        advanced: true,
    },
    Field {
        key: "FORCE_CLEAN_BUILD",
        label: "Щоразу чиста збірка",
        help: "Не використовувати попередню збірку для пришвидшення. Повільніше, але інколи допомагає після великих оновлень.",
        kind: Kind::Toggle,
        default: "0",
        advanced: true,
    },
    Field {
        key: "PLUGINS_STRICT",
        label: "Суворо з плагінами",
        help: "Скасувати встановлення, якщо хоч один вибраний плагін не зібрався. \
               Типово OBS встановлюється без такого плагіна, з попередженням.",
        kind: Kind::Toggle,
        default: "0",
        advanced: true,
    },
    Field {
        key: "OPT_LEVEL",
        label: "Рівень оптимізації",
        help: "2 — як у дистрибутива (рекомендовано). 3 — агресивніша оптимізація компілятора: інколи трохи швидше, \
               але OBS і плагіни з нею майже не тестують. Головний виграш дає «Під який процесор», а не цей рівень.",
        kind: Kind::Choice(&[("2", "-O2, рекомендовано"), ("3", "-O3, агресивніше")]),
        default: "2",
        advanced: true,
    },
    Field {
        key: "NV_HEADERS_TAG",
        label: "nv-codec-headers",
        help: "auto — найновіші заголовки NVENC, які знає драйвер (580xx → 13.0). Можна вказати тег, напр. \
               n13.0.19.1. Не змінюй без потреби.",
        kind: Kind::Text,
        default: "auto",
        advanced: true,
    },
    Field {
        key: "CEF_BUILD",
        label: "Версія CEF",
        help: "Порожньо — автоматично з вихідних кодів OBS (разом з офіційним SHA-256).",
        kind: Kind::Number { optional: true },
        default: "",
        advanced: true,
    },
    Field {
        key: "EXTRA_CMAKE_ARGS",
        label: "Додатково для CMake",
        help: "Додаткові параметри CMake для OBS, напр. -DENABLE_WEBSOCKET=OFF.",
        kind: Kind::Text,
        default: "",
        advanced: true,
    },
];

pub fn field(key: &str) -> Option<&'static Field> {
    FIELDS.iter().find(|f| f.key == key)
}

/// Перевіряє значення поля; повертає опис помилки.
pub fn validate(field: &Field, value: &str) -> Result<(), String> {
    if value.contains(['\n', '\r']) {
        return Err("значення не може містити перенесення рядка".into());
    }
    match field.kind {
        Kind::Number { optional } => {
            if value.is_empty() {
                return if optional { Ok(()) } else { Err("потрібне число".into()) };
            }
            match value.parse::<u32>() {
                Ok(n) if n > 0 => Ok(()),
                _ => Err("потрібне додатне ціле число".into()),
            }
        }
        Kind::Toggle => match value {
            "0" | "1" => Ok(()),
            _ => Err("значення 0 або 1".into()),
        },
        Kind::Choice(options) => {
            if options.iter().any(|(v, _)| *v == value) {
                Ok(())
            } else {
                Err(format!("одне з: {}", options.iter().map(|(v, _)| *v).collect::<Vec<_>>().join(", ")))
            }
        }
        Kind::Text => {
            if field.key != "EXTRA_CMAKE_ARGS" && value.chars().any(char::is_whitespace) {
                Err("без пробілів".into())
            } else if matches!(field.key, "OBS_REF" | "NV_HEADERS_TAG") && value.is_empty() {
                Err("не може бути порожнім".into())
            } else {
                Ok(())
            }
        }
    }
}

pub fn settings_path() -> PathBuf {
    match env::var_os("OBS_LEGACY_SETTINGS") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => script::config_dir().join("settings.env"),
    }
}

/// Розбирає рядок KEY=VALUE так само, як скрипт.
fn parse_line(line: &str) -> Option<(&str, String)> {
    let line = line.trim_end_matches('\r');
    let trimmed = line.trim_start();
    let (key, value) = trimmed.split_once('=')?;
    let valid_key = key.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid_key {
        return None;
    }
    let unquoted = if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"')) || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    };
    Some((key, unquoted))
}

#[derive(Debug, Clone)]
pub struct Settings {
    path: PathBuf,
    lines: Vec<String>,
}

impl Settings {
    pub fn load() -> Self {
        Self::load_from(settings_path())
    }

    pub fn load_from(path: PathBuf) -> Self {
        let lines = fs::read_to_string(&path).map(|s| s.lines().map(str::to_string).collect()).unwrap_or_default();
        Self { path, lines }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Значення з файлу; як і в скрипті, перемагає перше входження ключа.
    pub fn raw(&self, key: &str) -> Option<String> {
        self.lines.iter().find_map(|l| match parse_line(l) {
            Some((k, v)) if k == key => Some(v),
            _ => None,
        })
    }

    /// Значення з файлу або типове.
    pub fn get(&self, key: &str) -> String {
        self.raw(key).unwrap_or_else(|| field(key).map(|f| f.default.to_string()).unwrap_or_default())
    }

    /// Чи перевизначено ключ змінною середовища (вона має пріоритет у скрипті).
    pub fn env_override(key: &str) -> Option<String> {
        env::var(key).ok()
    }

    pub fn set(&mut self, key: &str, value: &str) {
        let new_line = format!("{key}={value}");
        if let Some(line) = self.lines.iter_mut().find(|l| parse_line(l).is_some_and(|(k, _)| k == key)) {
            *line = new_line;
        } else {
            if self.lines.is_empty() {
                self.lines.push("# Налаштування OBS-Legacy (KEY=VALUE). Їх читає і TUI obs-legacy, і скрипт".into());
                self.lines.push("# build-obs-nvenc-legacy-artix.sh. Змінні середовища мають пріоритет.".into());
            }
            self.lines.push(new_line);
        }
    }

    /// Чи малювати суцільний фон (UI_BACKGROUND; змінна середовища має пріоритет).
    pub fn solid_background(&self) -> bool {
        Self::env_override("UI_BACKGROUND").unwrap_or_else(|| self.get("UI_BACKGROUND")) != "0"
    }

    pub fn plugins(&self) -> Vec<String> {
        let raw = self.raw("OBS_PLUGINS").unwrap_or_else(|| DEFAULT_PLUGINS.to_string());
        let mut out: Vec<String> = Vec::new();
        for id in raw.split([',', ' ', '\t']).map(str::trim).filter(|s| !s.is_empty() && *s != "none") {
            if !out.iter().any(|x| x == id) {
                out.push(id.to_string());
            }
        }
        out
    }

    pub fn set_plugins(&mut self, ids: &[String]) {
        let value = if ids.is_empty() { "none".to_string() } else { ids.join(",") };
        self.set("OBS_PLUGINS", &value);
    }

    pub fn save(&self) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut body = self.lines.join("\n");
        body.push('\n');
        let tmp = self.path.with_extension(format!("env.tmp{}", std::process::id()));
        fs::write(&tmp, body)?;
        fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("obs-legacy-test-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("settings.env")
    }

    #[test]
    fn parses_like_the_script() {
        assert_eq!(parse_line("OBS_REF=\"32.2.2\""), Some(("OBS_REF", "32.2.2".into())));
        assert_eq!(parse_line("  KEEP_VERSIONS='3'"), Some(("KEEP_VERSIONS", "3".into())));
        assert_eq!(parse_line("EXTRA_CMAKE_ARGS=-DA=1 -DB=2"), Some(("EXTRA_CMAKE_ARGS", "-DA=1 -DB=2".into())));
        assert_eq!(parse_line("# comment"), None);
        assert_eq!(parse_line("1BAD=x"), None);
    }

    #[test]
    fn keeps_unknown_lines_and_updates_in_place() {
        let path = temp_path("roundtrip");
        fs::write(&path, "# мій коментар\nPLUGIN_REF_obs_multi_rtmp=0.7.4.5\nOBS_REF=stable\n").unwrap();
        let mut s = Settings::load_from(path.clone());
        s.set("OBS_REF", "33.0.0");
        s.set("KEEP_VERSIONS", "3");
        s.save().unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text, "# мій коментар\nPLUGIN_REF_obs_multi_rtmp=0.7.4.5\nOBS_REF=33.0.0\nKEEP_VERSIONS=3\n");
    }

    #[test]
    fn plugins_default_and_none() {
        let path = temp_path("plugins");
        let _ = fs::remove_file(&path);
        let mut s = Settings::load_from(path);
        assert_eq!(s.plugins(), vec!["obs-multi-rtmp".to_string()]);
        s.set_plugins(&[]);
        assert_eq!(s.get("OBS_PLUGINS"), "none");
        assert!(s.plugins().is_empty());
        s.set_plugins(&["a".into(), "b".into()]);
        assert_eq!(s.plugins(), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn basic_fields_come_first() {
        let first_advanced = FIELDS.iter().position(|f| f.advanced).unwrap();
        assert!(FIELDS[first_advanced..].iter().all(|f| f.advanced));
        assert!(FIELDS.iter().any(|f| f.key == "UI_BACKGROUND" && !f.advanced));
        let path = temp_path("background");
        fs::write(&path, "UI_BACKGROUND=0\n").unwrap();
        // Без змінної середовища вирішує файл.
        if env::var_os("UI_BACKGROUND").is_none() {
            assert!(!Settings::load_from(path).solid_background());
        }
    }

    #[test]
    fn validation() {
        let jobs = field("BUILD_JOBS").unwrap();
        assert!(validate(jobs, "").is_ok());
        assert!(validate(jobs, "4").is_ok());
        assert!(validate(jobs, "0").is_err());
        assert!(validate(jobs, "x").is_err());
        let keep = field("KEEP_VERSIONS").unwrap();
        assert!(validate(keep, "").is_err());
        let r = field("OBS_REF").unwrap();
        assert!(validate(r, "33.0.0").is_ok());
        assert!(validate(r, "a b").is_err());
        assert!(validate(r, "").is_err());
        assert!(validate(field("EXTRA_CMAKE_ARGS").unwrap(), "-DA=1 -DB=2").is_ok());
        let cpu = field("CPU_TUNING").unwrap();
        assert!(validate(cpu, "native").is_ok());
        assert!(validate(cpu, "x86-64-v3").is_ok());
        assert!(validate(cpu, "skylake").is_err());
        assert!(validate(cpu, "").is_err());
    }

    #[test]
    fn choices_cycle_and_defaults_are_valid() {
        let cpu = field("CPU_TUNING").unwrap();
        assert_eq!(cpu.kind.next_choice("native"), Some("generic"));
        assert_eq!(cpu.kind.next_choice("x86-64-v4"), Some("native"));
        assert_eq!(cpu.kind.next_choice("невідоме"), Some("native"));
        assert_eq!(cpu.kind.choice_label("native"), Some("саме під цей процесор"));
        assert_eq!(field("OPT_LEVEL").unwrap().kind.next_choice("2"), Some("3"));
        for f in FIELDS {
            if !matches!(f.kind, Kind::Text) || !f.default.is_empty() {
                assert!(validate(f, f.default).is_ok(), "типове значення {} некоректне", f.key);
            }
        }
    }
}
