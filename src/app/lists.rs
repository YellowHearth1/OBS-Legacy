//! Дії на екранах-списках: галочки плагінів, відкат на іншу збірку, зміна налаштувань.

use super::editor::Editor;
use super::menu::Action;
use super::{App, ToastKind};
use crate::catalog::PluginInfo;
use crate::settings::{self, Kind};
use crate::theme::Theme;

impl App {
    pub(super) fn toggle_plugin(&mut self, i: usize) {
        let Some(id) = self.catalog.as_ref().and_then(|c| c.get(i)).map(|p| p.id.clone()) else { return };
        let mut ids = self.settings.plugins();
        if let Some(pos) = ids.iter().position(|x| *x == id) {
            ids.remove(pos);
        } else {
            ids.push(id);
        }
        self.save_plugins(ids);
    }

    pub(super) fn set_plugins(&mut self, pred: impl Fn(&PluginInfo) -> bool) {
        let ids = self
            .catalog
            .as_ref()
            .map(|c| c.iter().filter(|p| pred(p)).map(|p| p.id.clone()).collect())
            .unwrap_or_default();
        self.save_plugins(ids);
    }

    fn save_plugins(&mut self, ids: Vec<String>) {
        // Порядок як у каталозі — так зручніше читати settings.env.
        let ordered: Vec<String> = match &self.catalog {
            Some(cat) => cat.iter().filter(|p| ids.contains(&p.id)).map(|p| p.id.clone()).collect(),
            None => ids,
        };
        self.settings.set_plugins(&ordered);
        match self.settings.save() {
            Ok(()) => self.toast(
                ToastKind::Ok,
                tr!(
                    "Збережено — застосується при наступному «Встановити / оновити OBS».",
                    "Saved — takes effect with the next “Install / update OBS”."
                ),
            ),
            Err(e) => self.toast(
                ToastKind::Err,
                trf!("Не вдалося зберегти налаштування: {e}", "Couldn't save the settings: {e}"),
            ),
        }
    }

    pub(super) fn rollback_to(&mut self, i: usize) {
        let Some(b) = self.builds.get(i) else { return };
        if b.active {
            self.toast(ToastKind::Info, tr!("Ця збірка вже активна.", "This build is already active."));
        } else if !b.complete {
            self.toast(
                ToastKind::Warn,
                tr!(
                    "Ця збірка неповна — на неї не можна перемкнутися.",
                    "This build is incomplete — you can't switch to it."
                ),
            );
        } else {
            let label = b.label.clone();
            self.start_script(Action::Rollback, &["rollback", &label]);
        }
    }

    pub(super) fn edit_setting(&mut self, i: usize) {
        let f = &settings::FIELDS[i];
        if f.kind == Kind::Toggle {
            let new = if self.settings.get(f.key) == "1" { "0" } else { "1" };
            self.commit_setting(i, new);
        } else if let Some(next) = f.kind.next_choice(&self.settings.get(f.key)) {
            self.commit_setting(i, next);
        } else {
            self.editor = Some(Editor::new(&self.settings.get(f.key)));
            self.settings_error = None;
        }
    }

    pub(super) fn commit_setting(&mut self, i: usize, value: &str) {
        let f = &settings::FIELDS[i];
        if let Err(e) = settings::validate(f, value) {
            self.settings_error = Some(e);
            return;
        }
        self.settings.set(f.key, value);
        match self.settings.save() {
            Ok(()) => {
                self.editor = None;
                self.settings_error = None;
                // Мова — одразу, щоб і підтвердження вже було нею.
                if f.key == "UI_LANG" {
                    crate::i18n::set(self.settings.ui_lang());
                }
                let label = f.label();
                self.toast(ToastKind::Ok, trf!("{label}: збережено", "{label}: saved"));
                match f.key {
                    "NV_HEADERS_TAG" => self.refresh(),
                    "UI_BACKGROUND" => self.theme = Theme::detect(self.settings.solid_background()),
                    // Описи плагінів дає скрипт — тією мовою, яку йому передали.
                    "UI_LANG" => self.spawn_catalog(),
                    _ => {}
                }
            }
            Err(e) => self.settings_error = Some(trf!("не вдалося зберегти: {e}", "couldn't save: {e}")),
        }
    }
}
