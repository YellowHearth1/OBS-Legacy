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
}

impl PluginInfo {
    /// Посилання на репозиторій без https:// і .git — для показу.
    pub fn short_repo(&self) -> &str {
        let r = self.repo.trim_start_matches("https://");
        r.strip_suffix(".git").unwrap_or(r)
    }
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
            })
        })
        .collect()
}

pub fn load(script: &Path) -> Result<Vec<PluginInfo>> {
    let out = Command::new("bash")
        .arg(script)
        .args(["plugins", "--tsv"])
        .env("OBS_LEGACY_SETTINGS", settings::settings_path())
        .output()
        .context(tr!("не вдалося запустити bash", "couldn't start bash"))?;
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
    }

    #[test]
    fn embedded_script_catalog_loads() {
        // Каталог вбудованого скрипта має розбиратися без помилок.
        let dir = std::env::temp_dir().join(format!("obs-legacy-cat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("script.sh");
        std::fs::write(&path, crate::script::SCRIPT).unwrap();
        let list = load(&path).unwrap();
        assert!(list.iter().any(|p| p.id == "obs-multi-rtmp" && p.default));
        assert!(list.iter().all(|p| p.repo.starts_with("https://github.com/")));
        assert!(list.iter().all(|p| p.package.ends_with("-legacy-580xx") && !p.package.contains(' ')));
        assert!(list.iter().any(|p| p.id == "rewards-theater" && p.package == "rewards-theater-obs-legacy-580xx"));
    }
}
