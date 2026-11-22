//! Фонові відомості: система, звіти, пакети, яких бракує, каталог плагінів і нові версії. Кожне
//! збирається в окремому потоці й приходить повідомленням `Msg`, тож інтерфейс не завмирає.

use std::process::{Command, Stdio};
use std::thread;

use super::{App, Screen, ToastKind};
use crate::catalog::{self, PluginInfo};
use crate::reports::{self, ReportInfo};
use crate::service;
use crate::settings;
use crate::system::{self, SysInfo};
use crate::updates;
use crate::versions::{self, Updates};

pub(super) enum Msg {
    Sys(Box<SysInfo>),
    Updates(Updates),
    Catalog(Result<Vec<PluginInfo>, String>),
    Reports(Vec<ReportInfo>),
    /// Пакети для збірки, яких бракує (None — скрипт не відповів).
    Deps(Option<MissingDeps>),
    /// Сповіщення про оновлення: Some(служба) — увімкнено.
    /// Служба сповіщень; `true` — її щойно ввімкнено типово (перший запуск).
    Notifier(Option<service::Backend>, bool),
}

/// Пакети для збірки, яких бракує.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct MissingDeps {
    /// Є в репозиторіях — їх поставить «Встановити пакети».
    pub repo: Vec<String>,
    /// Обов'язкові, але в репозиторіях їх немає.
    pub norepo: Vec<String>,
}

/// Розбирає вивід `deps --missing`: рядки «repo <пакет>» і «norepo <пакет>», решту пропускає.
fn parse_missing_deps(out: &str) -> MissingDeps {
    let mut missing = MissingDeps::default();
    for line in out.lines() {
        match line.split_once(' ') {
            Some(("repo", p)) => missing.repo.push(p.trim().to_string()),
            Some(("norepo", p)) => missing.norepo.push(p.trim().to_string()),
            _ => {}
        }
    }
    missing
}

impl App {
    pub(super) fn spawn_sys(&mut self) {
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
            let installed = service::installed().first().map(|p| p.backend);
            let msg = match installed {
                Some(b) => Msg::Notifier(Some(b), false),
                None => match service::enable_by_default() {
                    Some(b) => Msg::Notifier(Some(b), true),
                    None => Msg::Notifier(None, false),
                },
            };
            let _ = tx.send(msg);
        });
    }

    pub(super) fn spawn_reports(&mut self) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let _ = tx.send(Msg::Reports(reports::list()));
        });
    }

    /// Яких пакетів для збірки бракує (`deps --missing`, без sudo і швидко).
    pub(super) fn spawn_deps(&mut self) {
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
            let parsed = out
                .ok()
                .filter(|o| o.status.success())
                .map(|o| parse_missing_deps(&String::from_utf8_lossy(&o.stdout)));
            let _ = tx.send(Msg::Deps(parsed));
        });
    }

    pub(super) fn spawn_catalog(&mut self) {
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
            let _ = tx.send(Msg::Updates(versions::check_updates(&repos, Some(&self_repo))));
        });
    }

    pub(super) fn refresh(&mut self) {
        self.spawn_sys();
        self.spawn_reports();
        self.spawn_deps();
        self.updates = None;
        self.updates_requested = false;
    }

    pub(super) fn process_messages(&mut self) {
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
                Msg::Deps(Some(missing)) => {
                    self.missing_packages = Some(missing.repo);
                    self.missing_norepo = missing.norepo;
                    changed = true;
                }
                Msg::Deps(None) => {}
                Msg::Notifier(n, just_enabled) => {
                    if just_enabled && let Some(b) = n {
                        self.toast(
                            ToastKind::Ok,
                            trf!(
                                "Сповіщення про нові версії OBS і OBS-Legacy ввімкнено ({}). Вимкнути: Додатково → \
                                 «Сповіщення про оновлення».",
                                "Notifications about new OBS and OBS-Legacy versions are on ({}). To turn them off: \
                                 More → “Update notifications”.",
                                b.label()
                            ),
                        );
                    }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_deps_output() {
        let m = parse_missing_deps("repo swig\nnorepo libfoo\nщось інше\nrepo  asio \n\n");
        assert_eq!(m.repo, ["swig", "asio"]);
        assert_eq!(m.norepo, ["libfoo"]);
        assert_eq!(parse_missing_deps(""), MissingDeps::default());
    }
}
