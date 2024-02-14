use std::path::PathBuf;

use eframe::egui::{self, RichText};
use egui_file::FileDialog;
use eso_addons_core::service::AddonService;

use crate::views::View;
use crate::{REPO, VERSION};

use super::ui_helpers::{AddonResponse, PromisedValue};

#[derive(Default)]
pub struct Settings {
    opened_file: Option<PathBuf>,
    open_file_dialog: Option<FileDialog>,
    minion_import: Option<PromisedValue<()>>,
    open_addon_dir_dialog: Option<FileDialog>,
}
impl Settings {
    fn poll(&mut self) {
        // poll promises
        if self.minion_import.is_some() {
            self.minion_import.as_mut().unwrap().poll();
            if self.minion_import.as_ref().unwrap().is_ready() {
                // clear promise
                self.minion_import = None;
            }
        }
    }
}
impl View for Settings {
    fn ui(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        service: &mut AddonService,
    ) -> AddonResponse {
        self.poll();

        ui.add_space(5.0);
        ui.label(RichText::new("Game AddOn folder Path").heading());
        ui.horizontal(|ui| {
            if ui.button("🗁").clicked() {
                // select game addon path
                let mut dialog = FileDialog::select_folder(Some(service.config.addon_dir.clone()));
                dialog.open();
                self.open_addon_dir_dialog = Some(dialog);
            }
            ui.label(
                service
                    .config
                    .addon_dir
                    .clone()
                    .into_os_string()
                    .to_str()
                    .unwrap(),
            );
        });
        if let Some(dialog) = &mut self.open_addon_dir_dialog {
            if dialog.show(ctx).selected() {
                if let Some(dir) = dialog.path() {
                    service.config.addon_dir = dir.to_path_buf();
                    service.save_config();
                }
            }
        }
        ui.add_space(10.0);

        // ui.checkbox(
        //     // TODO: make this mean something, currently has no effect
        //     &mut service.config.update_on_launch,
        //     "Check for updates on launch",
        // );
        ui.label(RichText::new("Updates").heading());
        ui.checkbox(
            &mut service.config.update_ttc_pricetable,
            "Update TTC PriceTable on launch (requires TamrielTradeCentre to be installed)",
        );
        ui.checkbox(
            &mut service.config.update_hm_data,
            "Update HarvestMap data on launch (requires HarvestMap-Data to be installed)",
        );
        ui.add_space(10.0);

        ui.label(RichText::new("Import from Minion").heading());
        ui.label("How to import from Minion:");
        ui.label("Create a new backup in minion. Locate the backup folder and use that has a name like 'addons.txt'.");
        // TODO: add wiki page to github repo with info on how to backup, how to find backup folder
        // ui.label("For additional help, check this link.");
        if self.minion_import.is_none() {
            if ui.button("Import from Minion...").clicked() {
                let mut dialog = FileDialog::open_file(self.opened_file.clone());
                dialog.open();
                self.open_file_dialog = Some(dialog);
            }
        } else {
            ui.add_enabled(false, egui::Button::new("Importing..."));
            ui.spinner();
        }
        if let Some(dialog) = &mut self.open_file_dialog {
            if dialog.show(ctx).selected() {
                if let Some(file) = dialog.path() {
                    self.opened_file = Some(file.to_path_buf());
                    let mut promise = PromisedValue::<()>::default();
                    promise.set(service.import_minion_file(file));
                    self.minion_import = Some(promise);
                }
            }
        }
        ui.add_space(10.0);

        ui.label(RichText::new("About").heading());
        ui.label(format!("Version: {}", VERSION));
        if let Some(repo) = REPO {
            ui.hyperlink_to(" GitHub", repo);
        }

        AddonResponse::default()
    }
}
