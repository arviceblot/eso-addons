use std::path::PathBuf;

use eframe::egui::{self};
use egui_file::FileDialog;
use eso_addons_core::service::result::AddonDetails;
use eso_addons_core::service::AddonService;
use lazy_async_promise::{ImmediateValuePromise, ImmediateValueState};
use tokio::runtime::{Handle, Runtime};

use crate::views::View;
use crate::REPO;

#[derive(Default)]
pub struct Settings {
    opened_file: Option<PathBuf>,
    open_file_dialog: Option<FileDialog>,
    test_promise: Option<ImmediateValuePromise<AddonDetails>>,
    test_val: Option<String>,
}
impl View for Settings {
    fn ui(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, service: &mut AddonService) {
        let rt = Handle::current();
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

        if self.test_promise.is_some() {
            // let result = self.test_promise.unwrap();
            let promise = self.test_promise.as_mut().unwrap().poll_state();
            if let ImmediateValueState::Success(val) = promise {
                self.test_val = Some(val.name.to_string());
            }
        }
        if self.test_val.is_none() {
            if self.test_promise.is_none() {
                if ui.button("Promise").clicked() {
                    self.test_promise = Some(service.test_primse(7));
                    self.test_val = None;
                }
            } else {
                ui.spinner();
            }
        } else {
            ui.label(self.test_val.as_mut().unwrap().to_string());
        }

        if REPO.is_some() {
            ui.hyperlink_to("GitHub", REPO.unwrap());
        }
    }
}
