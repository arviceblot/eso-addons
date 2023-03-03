use std::path::PathBuf;

use eframe::egui::{self};
use egui_file::FileDialog;
use eso_addons_core::service::AddonService;

use crate::views::View;
use crate::REPO;

use super::ui_helpers::PromisedValue;

#[derive(Default)]
pub struct Settings {
    opened_file: Option<PathBuf>,
    open_file_dialog: Option<FileDialog>,
    minion_import: Option<PromisedValue<()>>,
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
    ) -> Option<i32> {
        self.poll();

        ui.checkbox(
            service.config.update_on_launch.get_or_insert(false),
            "Update on launch",
        );
        ui.checkbox(
            service.config.update_ttc_pricetable.get_or_insert(false),
            "Update TTC PriceTable",
        );
        ui.separator();

        if self.minion_import.is_none() {
            if ui.button("Import from Minion...").clicked() {
                let mut dialog = FileDialog::open_file(self.opened_file.clone());
                dialog.open();
                self.open_file_dialog = Some(dialog);
            }
        } else {
            ui.add_enabled(false, egui::Button::new("Importing..."));
            ui.spinner();
            if let Some(dialog) = &mut self.open_file_dialog {
                if dialog.show(ctx).selected() {
                    if let Some(file) = dialog.path() {
                        self.opened_file = Some(file);
                        let mut promise = PromisedValue::<()>::default();
                        promise.set(service.import_minion_file(self.opened_file.as_ref().unwrap()));
                        self.minion_import = Some(promise);
                    }
                }
            }
        }

        if REPO.is_some() {
            ui.hyperlink_to("GitHub", REPO.unwrap());
        }

        None
    }
}
