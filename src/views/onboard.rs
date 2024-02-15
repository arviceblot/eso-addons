use eframe::egui::{self, Layout, RichText};
use egui_file::FileDialog;
use eso_addons_core::config::detect_addon_dir;

use crate::views::View;

use super::ui_helpers::AddonResponse;

#[derive(Default)]
pub struct Onboard {
    open_addon_dir_dialog: Option<FileDialog>,
    addon_dir_set: bool,
    setup_done: bool,
}
impl Onboard {
    pub fn is_setup_done(&self) -> bool {
        self.addon_dir_set
    }
}
impl View for Onboard {
    fn ui(
        &mut self,
        ctx: &eframe::egui::Context,
        ui: &mut eframe::egui::Ui,
        service: &mut eso_addons_core::service::AddonService,
    ) -> AddonResponse {
        let response = AddonResponse::default();
        // welcome
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Welcome to the Unofficial ESO AddOn Manager!")
                    .heading()
                    .strong(),
            );
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                // import from minion option?
                // done?
                if ui
                    .add_enabled(
                        self.is_setup_done(),
                        egui::Button::new(RichText::new("Done!").heading()),
                    )
                    .clicked()
                {
                    self.setup_done = true;
                    service.config.onboard = false;
                    service.save_config();
                }
            });
        });
        ui.add_space(5.0);

        ui.heading("Let's start by finding the right foler to save your AddOns:");
        ui.add_space(5.0);
        // select game addon path
        if ui
            .button(RichText::new("Select ESO AddOn folder...").heading())
            .clicked()
        {
            let mut dialog = FileDialog::select_folder(Some(detect_addon_dir()));
            dialog.open();
            self.open_addon_dir_dialog = Some(dialog);
        }
        if let Some(dialog) = &mut self.open_addon_dir_dialog {
            if dialog.show(ctx).selected() {
                if let Some(dir) = dialog.path() {
                    service.config.addon_dir = dir.to_path_buf();
                    service.save_config();
                    self.addon_dir_set = true;
                }
            }
        }
        ui.add_space(5.0);
        ui.label(
            service
                .config
                .addon_dir
                .clone()
                .into_os_string()
                .to_str()
                .unwrap(),
        );

        response
    }
}
