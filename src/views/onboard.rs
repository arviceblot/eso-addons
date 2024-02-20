use std::path::PathBuf;

use eframe::egui::{self, Button, Layout, RichText};
use eso_addons_core::{config::detect_addon_dir, service::AddonService};
use lazy_async_promise::ImmediateValuePromise;
use rfd::AsyncFileDialog;

use crate::views::View;

use super::ui_helpers::{AddonResponse, PromisedValue};

#[derive(Default)]
pub struct Onboard {
    addon_dir_dialog: PromisedValue<Option<String>>,
    addon_dir_set: bool,
    setup_done: bool,
}
impl Onboard {
    fn poll(&mut self, service: &mut AddonService) {
        // poll change addon dir dialog
        self.addon_dir_dialog.poll();
        if self.addon_dir_dialog.is_ready() {
            self.addon_dir_dialog.handle();
            let value = self.addon_dir_dialog.value.as_ref().unwrap();
            if let Some(path) = value {
                service.config.addon_dir = PathBuf::from(path);
                service.save_config();
                self.addon_dir_set = true;
            } else {
                self.addon_dir_set = false;
            }
        }
    }
    pub fn is_setup_done(&self) -> bool {
        self.addon_dir_set
    }
}
impl View for Onboard {
    fn ui(
        &mut self,
        _ctx: &eframe::egui::Context,
        ui: &mut eframe::egui::Ui,
        service: &mut AddonService,
    ) -> AddonResponse {
        let response = AddonResponse::default();

        self.poll(service);

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
        if self.addon_dir_dialog.is_polling() {
            ui.add_enabled(
                false,
                Button::new(RichText::new("Select ESO AddOn folder...").heading()),
            );
        } else if ui
            .button(RichText::new("Select ESO AddOn folder...").heading())
            .clicked()
        {
            let promise = ImmediateValuePromise::new(async move {
                let dialog = AsyncFileDialog::new()
                    .set_directory(detect_addon_dir())
                    .pick_folder()
                    .await;
                if let Some(path) = dialog {
                    return Ok(Some(path.path().to_string_lossy().to_string()));
                }
                Ok(None::<String>)
            });
            self.addon_dir_dialog.set(promise);
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
