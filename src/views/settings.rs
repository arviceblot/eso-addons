use std::path::PathBuf;

use eframe::egui::{self};
use egui_file::FileDialog;
use eso_addons_core::service::AddonService;
use tokio::runtime::Runtime;

use crate::views::View;
use crate::REPO;

#[derive(Default)]
pub struct Settings {
    opened_file: Option<PathBuf>,
    open_file_dialog: Option<FileDialog>,
}
impl View for Settings {
    fn ui(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        rt: &Runtime,
        service: &mut AddonService,
    ) {
        ui.checkbox(
            service.config.update_on_launch.get_or_insert(false),
            "Update on launch",
        );
        ui.checkbox(
            service.config.update_ttc_pricetable.get_or_insert(false),
            "Update TTC PriceTable",
        );
        ui.separator();

        if ui.button("Import from Minion...").clicked() {
            let mut dialog = FileDialog::open_file(self.opened_file.clone());
            dialog.open();
            self.open_file_dialog = Some(dialog);
        }

        if let Some(dialog) = &mut self.open_file_dialog {
            if dialog.show(ctx).selected() {
                if let Some(file) = dialog.path() {
                    self.opened_file = Some(file);
                    rt.block_on(service.import_minion_file(self.opened_file.as_ref().unwrap()));
                }
            }
        }

        if REPO.is_some() {
            ui.hyperlink_to("GitHub", REPO.unwrap());
        }
    }
}
