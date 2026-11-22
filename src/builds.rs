//! Збірки OBS-Legacy: встановлена (відомості з пакетів у /usr/share/obs-legacy), набори пакетів у
//! сховищі (для встановлення й відкату) і збірки старої схеми в /opt. Лише читає.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::paths;
use crate::system::{info_dir, install_root};

#[derive(Debug, Clone, Default)]
pub struct BuildInfo {
    pub label: String,
    pub fields: BTreeMap<String, String>,
    pub complete: bool,
    pub active: bool,
    /// Збірка старої схеми (/opt), а не набір пакетів.
    pub legacy: bool,
}

impl BuildInfo {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }

    pub fn obs_version(&self) -> &str {
        self.get("obs_version").unwrap_or("?")
    }

    /// Дата збірки у вигляді «2026-10-28 14:05».
    pub fn built(&self) -> String {
        self.get("built").map(|b| b.replace('T', " ").chars().take(16).collect()).unwrap_or_default()
    }

    /// (id, версія) плагінів у цій збірці.
    pub fn plugins(&self) -> Vec<(String, String)> {
        self.fields
            .iter()
            .filter_map(|(k, v)| {
                let id = k.strip_prefix("plugin.")?;
                Some((id.to_string(), v.split_whitespace().next().unwrap_or("").to_string()))
            })
            .collect()
    }

    pub fn plugins_failed(&self) -> Vec<String> {
        self.get("plugins_failed").map(|s| s.split_whitespace().map(str::to_string).collect()).unwrap_or_default()
    }

    /// Під що оптимізовано: «ivybridge · -O2 · CUDA sm_61» (для старих збірок — None).
    pub fn profile(&self) -> Option<String> {
        let tuning = self.get("cpu_tuning").filter(|v| !v.is_empty())?;
        let cpu = match (tuning, self.get("cpu_march").filter(|v| !v.is_empty())) {
            ("native", Some(march)) => march.to_string(),
            (t, _) => t.to_string(),
        };
        let mut s = format!("{cpu} · -O{}", self.get("opt_level").unwrap_or("2"));
        if let Some(cc) = self.get("gpu_cc").filter(|v| !v.is_empty()) {
            s.push_str(&format!(" · CUDA sm_{}", cc.replace('.', "")));
        }
        Some(s)
    }
}

fn parse_buildinfo(text: &str) -> BTreeMap<String, String> {
    text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect()
}

/// Відомості встановленої збірки: BUILDINFO пакета OBS, версії плагінів з їхніх
/// пакетів і ffmpeg (ключі `ffmpeg.*`). None — пакети OBS-Legacy не встановлено.
fn installed_info_from(dir: &Path) -> Option<BTreeMap<String, String>> {
    let mut fields = parse_buildinfo(&fs::read_to_string(dir.join("BUILDINFO.txt")).ok()?);
    if let Ok(rd) = fs::read_dir(dir.join("plugins")) {
        for e in rd.filter_map(Result::ok) {
            if let Ok(t) = fs::read_to_string(e.path()) {
                fields.extend(parse_buildinfo(&t));
            }
        }
    }
    if let Ok(t) = fs::read_to_string(dir.join("ffmpeg.txt")) {
        fields.extend(parse_buildinfo(&t).into_iter().map(|(k, v)| (format!("ffmpeg.{k}"), v)));
    }
    Some(fields)
}

/// Підкаталоги з BUILDINFO.txt і позначкою .complete (набори пакетів чи збірки в /opt).
fn read_build_dirs(dir: &Path, active: Option<&str>, legacy: bool) -> Vec<BuildInfo> {
    fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| {
                    let label = e.file_name().to_string_lossy().into_owned();
                    let dir = e.path();
                    BuildInfo {
                        fields: fs::read_to_string(dir.join("BUILDINFO.txt"))
                            .map(|t| parse_buildinfo(&t))
                            .unwrap_or_default(),
                        complete: dir.join(".complete").exists(),
                        active: active == Some(label.as_str()),
                        label,
                        legacy,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Збірки для панелі стану й відкату: набори пакетів (активний — встановлений),
/// а поки пакетів немає — ще й збірки старої схеми в /opt.
pub fn read_builds() -> (Vec<BuildInfo>, Option<String>) {
    read_builds_from(&info_dir(), &paths::package_store(), &install_root())
}

fn read_builds_from(info: &Path, store: &Path, legacy_root: &Path) -> (Vec<BuildInfo>, Option<String>) {
    let installed = installed_info_from(info);
    let mut active = installed.as_ref().and_then(|f| f.get("label").cloned());
    let mut builds = read_build_dirs(store, active.as_deref(), false);
    if let (Some(fields), Some(label)) = (installed, active.clone()) {
        match builds.iter_mut().find(|b| b.label == label) {
            // Для встановленого набору — відомості з системи: плагін могли прибрати окремо.
            Some(b) => b.fields = fields,
            None => builds.push(BuildInfo { label, fields, complete: true, active: true, legacy: false }),
        }
    }
    if active.is_none() {
        active = fs::read_link(legacy_root.join("current"))
            .ok()
            .and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned()));
        builds.extend(read_build_dirs(&legacy_root.join("versions"), active.as_deref(), true));
    }
    builds.sort_by(|a, b| b.label.cmp(&a.label));
    (builds, active)
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::PathBuf;

    use super::*;
    use crate::system::SysInfo;

    #[test]
    fn buildinfo_parsing() {
        let b = BuildInfo {
            fields: parse_buildinfo(
                "label=x\nobs_version=33.0.0\nbuilt=2026-10-28T14:05:11+02:00\nplugin.obs-multi-rtmp=0.8.0.0 abc123\nplugins_failed=waveform\n",
            ),
            ..Default::default()
        };
        assert_eq!(b.obs_version(), "33.0.0");
        assert_eq!(b.built(), "2026-10-28 14:05");
        assert_eq!(b.plugins(), [("obs-multi-rtmp".to_string(), "0.8.0.0".to_string())]);
        assert_eq!(b.plugins_failed(), ["waveform"]);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("obs-legacy-sys-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn packages_and_legacy_builds() {
        let root = temp_dir("builds");
        let (info, store, legacy) = (root.join("info"), root.join("store"), root.join("opt"));
        write(&legacy.join("versions/20261029-093154_33.0.0/BUILDINFO.txt"), "obs_version=33.0.0\n");
        write(&legacy.join("versions/20261029-093154_33.0.0/.complete"), "");
        std::os::unix::fs::symlink("versions/20261029-093154_33.0.0", legacy.join("current")).unwrap();
        write(
            &store.join("20261029-180000_33.0.0/BUILDINFO.txt"),
            "label=20261029-180000_33.0.0\nobs_version=33.0.0\n",
        );
        write(&store.join("20261029-180000_33.0.0/.complete"), "");

        // Пакетів ще немає: набір зібрано, але активна — стара збірка в /opt.
        let (builds, active) = read_builds_from(&info, &store, &legacy);
        assert_eq!(active.as_deref(), Some("20261029-093154_33.0.0"));
        assert_eq!(builds.len(), 2);
        assert!(builds.iter().any(|b| b.legacy && b.active));
        assert!(builds.iter().any(|b| !b.legacy && !b.active && b.complete));

        // Пакети встановлено: активний — набір, плагіни й ffmpeg — з відомостей у системі, /opt не видно.
        write(&info.join("BUILDINFO.txt"), "label=20261029-180000_33.0.0\nobs_version=33.0.0\ncpu_tuning=native\n");
        write(&info.join("plugins/obs-multi-rtmp"), "plugin.obs-multi-rtmp=0.8.0.0 f23d759786b7\n");
        write(&info.join("ffmpeg.txt"), "arch_version=2:9.0.2-2\n");
        let (builds, active) = read_builds_from(&info, &store, &legacy);
        assert_eq!(active.as_deref(), Some("20261029-180000_33.0.0"));
        assert_eq!(builds.len(), 1);
        let b = &builds[0];
        assert!(b.active && !b.legacy);
        assert_eq!(b.plugins(), [("obs-multi-rtmp".to_string(), "0.8.0.0".to_string())]);
        assert_eq!(b.get("ffmpeg.arch_version"), Some("2:9.0.2-2"));

        // Набір зі сховища видалили — встановлена збірка однаково видна.
        fs::remove_dir_all(&store).unwrap();
        let (builds, _) = read_builds_from(&info, &store, &legacy);
        assert!(builds.len() == 1 && builds[0].active && builds[0].complete);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn profile_and_cpu_change() {
        let b = BuildInfo {
            fields: parse_buildinfo(
                "cpu_tuning=native\ncpu_march=ivybridge\nopt_level=2\ngpu_cc=6.1\ncpu_model=Intel Xeon E3-1275 V2\n",
            ),
            active: true,
            complete: true,
            ..Default::default()
        };
        assert_eq!(b.profile().as_deref(), Some("ivybridge · -O2 · CUDA sm_61"));
        let mut s = SysInfo { builds: vec![b], cpu_model: "Intel Xeon E3-1275 V2".into(), ..Default::default() };
        assert_eq!(s.built_for_other_cpu(), None);
        s.cpu_model = "AMD Ryzen 5 5600".into();
        assert_eq!(s.built_for_other_cpu().as_deref(), Some("Intel Xeon E3-1275 V2"));
        // Переносна збірка (generic) працює на будь-якому процесорі.
        s.builds[0].fields.insert("cpu_tuning".into(), "generic".into());
        assert_eq!(s.built_for_other_cpu(), None);
        assert_eq!(s.builds[0].profile().as_deref(), Some("generic · -O2 · CUDA sm_61"));
        assert_eq!(BuildInfo::default().profile(), None);
    }
}
