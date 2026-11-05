//! Тексти сповіщень: про нові версії OBS, плагінів, ffmpeg і самого OBS-Legacy та про бібліотеки, яких
//! бракує власній збірці, — і відбиток, щоб про те саме не сповіщати двічі.

use std::path::Path;

use super::state::State;
use super::{Facts, RELEASES, plugin_list};
use crate::notify;

/// Одне сповіщення.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// Про що саме: (ключ, значення) — версія або відбиток проблеми.
    pub items: Vec<(String, String)>,
    pub title: String,
    pub body: String,
    pub urgent: bool,
}

impl Notice {
    /// Чи є в ньому щось, про що ще не сповіщали.
    pub fn is_new(&self, st: &State) -> bool {
        self.items.iter().any(|(k, v)| st.get(&format!("notified.{k}")) != Some(v.as_str()))
    }

    pub fn mark(&self, st: &mut State) {
        for (k, v) in &self.items {
            st.set(&format!("notified.{k}"), v);
        }
    }

    pub fn message<'a>(&'a self, open: Option<&'a Path>) -> notify::Message<'a> {
        notify::Message { title: &self.title, body: &self.body, urgent: self.urgent, open }
    }
}

/// Як оновити OBS (і плагіни, і ffmpeg) — пункт меню.
fn how_install() -> &'static str {
    tr!("OBS-Legacy → «Встановити / оновити OBS»", "OBS-Legacy → “Install / update OBS”")
}

/// Сповіщення про нові версії (без бібліотек).
pub fn notices(f: &Facts) -> Vec<Notice> {
    let mut out = Vec::new();
    let plugin_items = || f.plugin_updates.iter().map(|p| (format!("plugin.{}", p.id), p.latest.clone()));
    if let (Some(latest), Some(installed)) = (f.obs_update(), f.obs_installed.as_deref()) {
        let how = how_install();
        let mut body = trf!(
            "У тебе OBS {installed}. Онови: {how}. Нова збірка ввімкнеться лише після перевірки NVENC, \
             а поточна лишиться для відкату.",
            "You have OBS {installed}. Update: {how}. The new build is switched on only after the NVENC check, \
             and the current one stays for rollback."
        );
        if !f.plugin_updates.is_empty() {
            let list = plugin_list(&f.plugin_updates);
            body.push_str(&trf!(
                " Разом з OBS оновляться плагіни: {list}.",
                " Plugins are updated together with OBS: {list}."
            ));
        }
        let mut items = vec![("obs".to_string(), latest.to_string())];
        items.extend(plugin_items());
        out.push(Notice {
            items, title: trf!("Вийшов OBS {latest}", "OBS {latest} is out"), body, urgent: false
        });
    } else if !f.plugin_updates.is_empty() {
        let (title, what) = match f.plugin_updates.as_slice() {
            [p] => (
                trf!("Нова версія плагіна {}: {}", "New version of the {} plugin: {}", p.name, p.latest),
                trf!("У тебе {}", "You have {}", p.installed),
            ),
            many => {
                (trf!("Нові версії плагінів OBS: {}", "New OBS plugin versions: {}", many.len()), plugin_list(many))
            }
        };
        let how = how_install();
        out.push(Notice {
            items: plugin_items().collect(),
            title,
            body: trf!(
                "{what}. Плагіни оновлюються разом з OBS: {how}.",
                "{what}. Plugins are updated together with OBS: {how}."
            ),
            urgent: false,
        });
    }
    if let Some(new) = &f.ffmpeg_new {
        let built = f.ffmpeg_built.as_deref().unwrap_or(tr!("старішої версії", "an older version"));
        let how = how_install();
        out.push(Notice {
            items: vec![("ffmpeg".into(), new.clone())],
            title: trf!(
                "Новий ffmpeg {new} — перезбери ffmpeg-legacy-580xx",
                "New ffmpeg {new} — rebuild ffmpeg-legacy-580xx"
            ),
            body: trf!(
                "Твій ffmpeg-legacy-580xx зібрано з {built}. Перезбери його з NVENC під свій драйвер: {how} \
                 (ffmpeg збирається ~15 хв). До того працює поточний.",
                "Your ffmpeg-legacy-580xx was built from {built}. Rebuild it with NVENC for your driver: {how} \
                 (ffmpeg takes ~15 min to build). Until then the current one works."
            ),
            urgent: false,
        });
    }
    if let Some(latest) = f.self_update() {
        let file = format!("obs-legacy-{}-1-x86_64.pkg.tar.zst", latest.trim_start_matches('v'));
        let how = if f.self_menu {
            tr!(
                "Онови: OBS-Legacy → Додатково → «Оновити OBS-Legacy».",
                "Update: OBS-Legacy → More → “Update OBS-Legacy”."
            )
            .to_string()
        } else if f.self_package {
            trf!(
                "Завантаж пакет {file} з {RELEASES} і постав: sudo pacman -U {file}",
                "Download the {file} package from {RELEASES} and install it: sudo pacman -U {file}"
            )
        } else {
            tr!(
                "Онови так само, як встановлював: git pull і ./install.sh у теці з вихідними кодами.",
                "Update the way you installed it: git pull and ./install.sh in the source folder."
            )
            .to_string()
        };
        let mine = &f.self_version;
        out.push(Notice {
            items: vec![("self".into(), latest.to_string())],
            title: trf!("Вийшов OBS-Legacy {}", "OBS-Legacy {} is out", latest.trim_start_matches('v')),
            body: trf!(
                "У тебе {mine}. {how} Твій OBS не зміниться.",
                "You have {mine}. {how} Your OBS stays as it is."
            ),
            urgent: false,
        });
    }
    out
}

/// Відбиток списку проблем (FNV-1a): сповіщати знову, лише коли він змінився.
fn fingerprint(problems: &[String]) -> String {
    let mut sorted: Vec<&str> = problems.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in sorted.join("\n").bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Сповіщення про бібліотеки, яких бракує власній збірці.
pub fn libs_notice(problems: &[String]) -> Option<Notice> {
    if problems.is_empty() {
        return None;
    }
    let mut libs: Vec<&str> = problems.iter().map(|p| p.rsplit_once(": ").map_or(p.as_str(), |(_, l)| l)).collect();
    libs.sort_unstable();
    libs.dedup();
    let mut list = libs.iter().take(3).copied().collect::<Vec<_>>().join(", ");
    if libs.len() > 3 {
        let more = libs.len() - 3;
        list.push_str(&trf!(" і ще {more}", " and {more} more"));
    }
    Some(Notice {
        items: vec![("libs".into(), fingerprint(problems))],
        title: tr!("OBS після оновлення системи може не запуститися", "OBS may not start after the system update")
            .into(),
        body: trf!(
            "Власній збірці OBS бракує бібліотек: {list}. Перезбери: {}.",
            "Your OBS build is missing libraries: {list}. Rebuild: {}.",
            how_install()
        ),
        urgent: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updates::{PluginUpdate, parse_libcheck};

    fn facts() -> Facts {
        Facts {
            obs_installed: Some("33.0.0".into()),
            obs_latest: Some("33.0.0".into()),
            plugins_checked: 2,
            self_version: "0.0.1".into(),
            self_latest: Some("v0.0.1".into()),
            self_menu: true,
            ..Default::default()
        }
    }

    fn blur(latest: &str) -> PluginUpdate {
        PluginUpdate {
            id: "composite-blur".into(),
            name: "Composite Blur".into(),
            installed: "v1.6.1".into(),
            latest: latest.into(),
        }
    }

    #[test]
    fn nothing_new() {
        let f = facts();
        assert!(notices(&f).is_empty());
        assert_eq!(f.summary(), "нового немає");
        let offline = Facts { obs_latest: None, ..facts() };
        assert!(!offline.online());
        assert!(offline.summary().starts_with("не вдалося перевірити"));
    }

    #[test]
    fn obs_update_carries_plugins() {
        let f = Facts { obs_latest: Some("33.0.1".into()), plugin_updates: vec![blur("v1.6.2")], ..facts() };
        let n = notices(&f);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].title, "Вийшов OBS 33.0.1");
        assert!(n[0].body.contains("У тебе OBS 33.0.0"));
        assert!(n[0].body.contains("Composite Blur v1.6.2"));
        assert_eq!(n[0].items, [("obs".into(), "33.0.1".into()), ("plugin.composite-blur".into(), "v1.6.2".into())]);
        assert_eq!(f.summary(), "вийшов OBS 33.0.1; нові версії плагінів: Composite Blur v1.6.2");
    }

    #[test]
    fn pinned_or_older_obs_is_quiet() {
        let pinned = Facts { obs_latest: Some("33.0.1".into()), obs_pinned: Some("32.2.2".into()), ..facts() };
        assert!(notices(&pinned).is_empty());
        let older = Facts { obs_latest: Some("32.2.2".into()), ..facts() };
        assert!(notices(&older).is_empty());
        let no_build = Facts { obs_installed: None, obs_latest: Some("33.0.1".into()), ..facts() };
        assert!(notices(&no_build).is_empty());
    }

    #[test]
    fn plugin_only_updates() {
        let one = notices(&Facts { plugin_updates: vec![blur("v1.6.2")], ..facts() });
        assert_eq!(one[0].title, "Нова версія плагіна Composite Blur: v1.6.2");
        assert!(one[0].body.starts_with("У тебе v1.6.1."));
        let wave = PluginUpdate {
            id: "waveform".into(),
            name: "Waveform".into(),
            installed: "v1.9.1".into(),
            latest: "v1.10.0".into(),
        };
        let two = notices(&Facts { plugin_updates: vec![blur("v1.6.2"), wave], ..facts() });
        assert_eq!(two[0].title, "Нові версії плагінів OBS: 2");
        assert!(two[0].body.starts_with("Composite Blur v1.6.2, Waveform v1.10.0."));
    }

    #[test]
    fn self_update() {
        let n = notices(&Facts { self_latest: Some("v0.0.2".into()), ..facts() });
        assert_eq!(n[0].title, "Вийшов OBS-Legacy 0.0.2");
        assert!(n[0].body.contains("«Оновити OBS-Legacy»"));
        let n = notices(&Facts { self_latest: Some("v0.0.2".into()), self_menu: false, ..facts() });
        assert!(n[0].body.contains("./install.sh"));
        let n = notices(&Facts { self_latest: Some("v0.0.3".into()), self_menu: false, self_package: true, ..facts() });
        assert!(n[0].body.contains("sudo pacman -U obs-legacy-0.0.3-1-x86_64.pkg.tar.zst"));
    }

    #[test]
    fn ffmpeg_update() {
        let f = Facts { ffmpeg_built: Some("2:9.0.2-2".into()), ffmpeg_new: Some("2:9.0.3-1".into()), ..facts() };
        let n = notices(&f);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].items, [("ffmpeg".to_string(), "2:9.0.3-1".to_string())]);
        assert!(n[0].title.contains("2:9.0.3-1") && n[0].body.contains("2:9.0.2-2"));
        assert!(f.summary().contains("новий ffmpeg 2:9.0.3-1"));
        // Зібрано з поточної версії — новин немає.
        assert!(notices(&Facts { ffmpeg_built: Some("2:9.0.2-2".into()), ..facts() }).is_empty());
    }

    #[test]
    fn notified_once() {
        let f = Facts { obs_latest: Some("33.0.1".into()), plugin_updates: vec![blur("v1.6.2")], ..facts() };
        let mut st = State::default();
        let n = &notices(&f)[0];
        assert!(n.is_new(&st));
        n.mark(&mut st);
        assert!(!n.is_new(&st));
        // Новий реліз плагіна — нове сповіщення.
        let f2 = Facts { plugin_updates: vec![blur("v1.6.3")], ..f };
        assert!(notices(&f2)[0].is_new(&st));
        // Стан переживає збереження й читання.
        let text: String = st.map.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
        assert!(!n.is_new(&State::parse(&format!("# коментар\n{text}"))));
    }

    #[test]
    fn libcheck_output() {
        let out = "\n==> OBS-Legacy: власній збірці OBS після оновлення бракує бібліотек:\n      \
                   bin/obs: libavcodec.so.61\n      lib/libobs.so.30: libavcodec.so.61\n      \
                   lib/obs-plugins/x.so: libQt6Core.so.6 (Qt_6.9)\n      … і ще 2\n    Перезбери OBS: x\n";
        let p = parse_libcheck(out);
        assert_eq!(p.len(), 3);
        let n = libs_notice(&p).unwrap();
        assert!(n.urgent);
        assert!(n.body.contains("libQt6Core.so.6 (Qt_6.9), libavcodec.so.61."));
        // Відбиток не залежить від порядку.
        let mut rev = p.clone();
        rev.reverse();
        assert_eq!(fingerprint(&p), fingerprint(&rev));
        assert!(libs_notice(&[]).is_none());
        assert!(parse_libcheck("").is_empty());
    }
}
