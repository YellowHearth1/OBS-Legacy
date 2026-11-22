//! Каталог плагінів. Єдине джерело правди — сам скрипт (`plugins --tsv`),
//! тож TUI завжди показує саме те, що скрипт уміє зібрати.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::settings;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub default: bool,
    pub repo: String,
    pub packages: String,
    pub description: String,
    /// Назва пакета pacman: <назва в AUR>-legacy-580xx.
    pub package: String,
    /// Свій плагін власника з файлу plugins.local, а не з вбудованого каталогу.
    pub local: bool,
    /// Назви пакета в AUR. Плагіни зі спільною назвою замінюють один одного.
    pub names: Vec<String>,
}

impl PluginInfo {
    /// Посилання на репозиторій без https:// і .git — для показу.
    pub fn short_repo(&self) -> &str {
        let r = self.repo.trim_start_matches("https://");
        r.strip_suffix(".git").unwrap_or(r)
    }

    /// Чи замінює цей плагін інший: той самий плагін у двох втіленнях (спільна назва в AUR),
    /// тож разом їх не вибирають.
    pub fn replaces(&self, other: &PluginInfo) -> bool {
        self.id != other.id && self.names.iter().any(|name| other.names.contains(name))
    }
}

/// Вибір без плагінів, що замінюють один одного. `ticked` — щойно позначений плагін: він лишається,
/// а ті, що він замінює, знімаються. Без нього з кожної такої пари лишається пізніший у каталозі
/// (свої плагіни йдуть після вбудованих). Повертає вибір і назви знятих плагінів.
pub fn without_replaced(catalog: &[PluginInfo], ids: &[String], ticked: Option<&str>) -> (Vec<String>, Vec<String>) {
    let chosen: Vec<&PluginInfo> = catalog.iter().filter(|p| ids.contains(&p.id)).collect();
    let mut kept: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    for (i, plugin) in chosen.iter().enumerate() {
        let beaten = chosen.iter().enumerate().any(|(j, other)| {
            other.replaces(plugin)
                && match ticked {
                    Some(id) if id == other.id => true,
                    Some(id) if id == plugin.id => false,
                    _ => j > i,
                }
        });
        if beaten {
            removed.push(plugin.name.clone());
        } else {
            kept.push(plugin.id.clone());
        }
    }
    // Ідентифікатори, яких немає в каталозі, лишаються як були: про них скаже сам скрипт.
    kept.extend(ids.iter().filter(|id| !catalog.iter().any(|p| &p.id == *id)).cloned());
    (kept, removed)
}

pub fn parse_tsv(text: &str) -> Vec<PluginInfo> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 7 || f[0].is_empty() {
                return None;
            }
            Some(PluginInfo {
                id: f[0].to_string(),
                name: f[1].to_string(),
                default: f[3] == "1",
                repo: f[4].to_string(),
                packages: f[5].to_string(),
                description: f[6].to_string(),
                package: f.get(7).map_or_else(|| format!("obs-{}-legacy-580xx", f[0]), |p| (*p).to_string()),
                local: f.get(8) == Some(&"1"),
                names: f.get(9).map(|names| names.split_whitespace().map(str::to_string).collect()).unwrap_or_default(),
            })
        })
        .collect()
}

pub fn load(script: &Path) -> Result<Vec<PluginInfo>> {
    load_with(script, None)
}

/// `local_file` — інший файл своїх плагінів замість plugins.local поруч із налаштуваннями (для тестів).
fn load_with(script: &Path, local_file: Option<&Path>) -> Result<Vec<PluginInfo>> {
    let mut cmd = Command::new("bash");
    cmd.arg(script)
        .args(["plugins", "--tsv"])
        .env("OBS_LEGACY_SETTINGS", settings::settings_path())
        .env("OBS_LEGACY_LANG", crate::i18n::lang().code());
    if let Some(file) = local_file {
        cmd.env("OBS_LEGACY_PLUGINS_LOCAL", file);
    }
    let out = cmd.output().context(tr!("не вдалося запустити bash", "couldn't start bash"))?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let list = parse_tsv(&String::from_utf8_lossy(&out.stdout));
    if list.is_empty() {
        bail!(tr!("скрипт повернув порожній каталог плагінів", "the script returned an empty plugin catalog"));
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_script_output() {
        let tsv = "obs-multi-rtmp\tMultiple RTMP Outputs\t1\t1\thttps://github.com/sorayuki/obs-multi-rtmp.git\t\tСтрім одночасно\tobs-multi-rtmp-legacy-580xx\n\
                   waveform\tWaveform\t0\t0\thttps://github.com/phandasm/waveform.git\tfftw\tВізуалізатор\n";
        let list = parse_tsv(tsv);
        assert_eq!(list.len(), 2);
        assert!(list[0].default);
        assert_eq!(list[0].short_repo(), "github.com/sorayuki/obs-multi-rtmp");
        assert_eq!(list[1].packages, "fftw");
        assert_eq!(list[1].description, "Візуалізатор");
        assert_eq!(list[0].package, "obs-multi-rtmp-legacy-580xx");
        assert_eq!(list[1].package, "obs-waveform-legacy-580xx");
        assert!(!list[0].local && !list[1].local);
    }

    #[test]
    fn embedded_script_catalog_loads() {
        // Каталог вбудованого скрипта має розбиратися без помилок. Свої плагіни власника машини
        // (plugins.local) тест не бачить — лише вбудований каталог.
        let dir = std::env::temp_dir().join(format!("obs-legacy-cat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("script.sh");
        std::fs::write(&path, crate::script::SCRIPT).unwrap();
        let list = load_with(&path, Some(&dir.join("no-such-plugins.local"))).unwrap();
        assert!(list.iter().all(|p| !p.local));
        assert!(list.iter().any(|p| p.id == "obs-multi-rtmp" && p.default));
        assert!(list.iter().all(|p| p.repo.starts_with("https://github.com/")));
        assert!(list.iter().all(|p| p.package.ends_with("-legacy-580xx") && !p.package.contains(' ')));
        assert!(list.iter().any(|p| p.id == "rewards-theater" && p.package == "rewards-theater-obs-legacy-580xx"));
    }

    #[test]
    fn own_plugins_come_from_plugins_local() {
        let dir = std::env::temp_dir().join(format!("obs-legacy-own-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("script.sh");
        std::fs::write(&script, crate::script::SCRIPT).unwrap();
        let local = dir.join("plugins.local");
        std::fs::write(
            &local,
            "# свій форк\n\
             my-fork|My Fork|/home/u/My-Fork|rust|-DX=ON|Свій плагін|my-fork-obs other-obs\n\
             rewards-theater|Copy|https://example.org/x.git||||\n\
             broken line\n",
        )
        .unwrap();
        let list = load_with(&script, Some(&local)).unwrap();
        let own: Vec<_> = list.iter().filter(|p| p.local).collect();
        assert_eq!(own.len(), 1, "{own:?}");
        assert_eq!(own[0].id, "my-fork");
        assert_eq!(own[0].repo, "/home/u/My-Fork");
        assert_eq!(own[0].packages, "rust");
        assert_eq!(own[0].package, "my-fork-obs-legacy-580xx");
        // Той самий id, що у вбудованому каталозі, не підміняє вбудований плагін.
        let rewards: Vec<_> = list.iter().filter(|p| p.id == "rewards-theater").collect();
        assert_eq!(rewards.len(), 1);
        assert_eq!(own[0].names, ["my-fork-obs", "other-obs"]);
        assert!(!own[0].replaces(rewards[0]), "no shared package name, no replacement");
        assert!(!rewards[0].local && rewards[0].name == "RewardsTheater");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn plugin(id: &str, names: &[&str]) -> PluginInfo {
        PluginInfo {
            id: id.into(),
            name: id.to_uppercase(),
            default: false,
            repo: String::new(),
            packages: String::new(),
            description: String::new(),
            package: format!("{id}-legacy-580xx"),
            local: false,
            names: names.iter().map(|n| (*n).to_string()).collect(),
        }
    }

    #[test]
    fn a_plugin_and_its_replacement_are_never_chosen_together() {
        let catalog = vec![
            plugin("multi", &["obs-multi-rtmp"]),
            plugin("old", &["old-obs", "old-obs-git"]),
            plugin("new", &["new-obs", "old-obs", "old-obs-git"]),
        ];
        assert!(catalog[2].replaces(&catalog[1]) && catalog[1].replaces(&catalog[2]));
        assert!(!catalog[0].replaces(&catalog[1]));
        let ids = |list: &[&str]| list.iter().map(|id| (*id).to_string()).collect::<Vec<_>>();
        // Щойно позначений лишається, інший знімається.
        let (kept, removed) = without_replaced(&catalog, &ids(&["multi", "old", "new"]), Some("new"));
        assert_eq!(kept, ["multi", "new"]);
        assert_eq!(removed, ["OLD"]);
        let (kept, removed) = without_replaced(&catalog, &ids(&["multi", "old", "new"]), Some("old"));
        assert_eq!(kept, ["multi", "old"]);
        assert_eq!(removed, ["NEW"]);
        // Без щойно позначеного лишається пізніший у каталозі (свій плагін).
        let (kept, _) = without_replaced(&catalog, &ids(&["old", "new", "unknown"]), None);
        assert_eq!(kept, ["new", "unknown"]);
        // Якщо вибрано лише один, нічого не змінюється.
        assert_eq!(without_replaced(&catalog, &ids(&["multi", "old"]), Some("old")).1, Vec::<String>::new());
    }

    #[test]
    fn script_output_carries_the_package_names() {
        let tsv = "a\tA\t0\t0\thttps://x/a.git\t\tОпис\ta-obs-legacy-580xx\t0\ta-obs b-obs\n";
        assert_eq!(parse_tsv(tsv)[0].names, ["a-obs", "b-obs"]);
        let old_script = "a\tA\t0\t0\thttps://x/a.git\t\tОпис\n";
        assert!(parse_tsv(old_script)[0].names.is_empty());
    }
}
