use std::path::{Path, PathBuf};

use eframe::egui::{self, Button, RichText, ScrollArea, Visuals};
use eso_addons_core::config;
use eso_addons_core::service::AddonService;
use lazy_async_promise::ImmediateValuePromise;
use rfd::AsyncFileDialog;

use crate::views::View;
use crate::{REPO, VERSION};

use super::ui_helpers::{AddonResponse, AddonResponseType, PromisedValue};

#[derive(Default)]
pub struct Settings {
    // opened_file: Option<PathBuf>,
    addon_dir_dialog: PromisedValue<Option<String>>,
    minion_dialog: PromisedValue<Option<String>>,
    minion_import: Option<PromisedValue<()>>,
    visuals: Visuals,
    // open_addon_dir_dialog: Option<FileDialog>,
}
impl Settings {
    fn poll(&mut self, service: &mut AddonService) -> AddonResponse {
        let mut response = AddonResponse::default();

        // poll promises

        // poll change addon dir dialog
        self.addon_dir_dialog.poll();
        if self.addon_dir_dialog.is_ready() {
            self.addon_dir_dialog.handle();
            let value = self.addon_dir_dialog.value.as_ref().unwrap();
            if let Some(path) = value {
                service.config.addon_dir = PathBuf::from(path);
                service.save_config();
            }
        }

        // poll minion file dialog
        self.minion_dialog.poll();
        if self.minion_dialog.is_ready() {
            self.minion_dialog.handle();
            let value = self.minion_dialog.value.as_ref().unwrap();
            // start import process if we got a file
            // TODO: Consider some path checks here? Maybe not...
            if let Some(path) = value {
                let mut promise = PromisedValue::<()>::default();
                promise.set(service.import_minion_file(Path::new(path)));
                self.minion_import = Some(promise);
            }
        }

        // poll minion import
        if self.minion_import.is_some() {
            self.minion_import.as_mut().unwrap().poll();
            if self.minion_import.as_ref().unwrap().is_ready() {
                // clear promise
                self.minion_import = None;
                response.response_type = AddonResponseType::AddonsChanged;
            }
        }

        response
    }
}
impl View for Settings {
    fn ui(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        service: &mut AddonService,
    ) -> AddonResponse {
        let response = self.poll(service);
        ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                if ui.selectable_label(service.config.style == config::Style::Light, RichText::new("☀ Light").heading()).clicked() {
                    service.config.style = config::Style::Light;
                    ctx.style_mut(|style| {
                        style.visuals = Visuals::light();
                    });
                } else if ui.selectable_label(service.config.style == config::Style::Dark, RichText::new("🌙 Dark").heading()).clicked() {
                    service.config.style = config::Style::Dark;
                    ctx.style_mut(|style| {
                        style.visuals = Visuals::dark();
                    });
                } else if ui.selectable_label(service.config.style == config::Style::System, RichText::new("Follow System").heading()).clicked() {
                    service.config.style = config::Style::System;
                }
                ui.label("requires app restart to take effect")
            });
            ui.add_space(5.0);

            ui.label(RichText::new("Game AddOn folder Path").heading());
            ui.add_space(5.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    "Note: changing the addon directory will not move any previously installed addons!",
                );
            });
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                if self.addon_dir_dialog.is_polling() {
                    // disabled button
                    ui.add_enabled(false, Button::new(RichText::new("🗁 Change").heading()));
                } else if ui.button(RichText::new("🗁 Change").heading()).clicked() {
                    let promise = ImmediateValuePromise::new(async move {
                        let dialog = AsyncFileDialog::new()
                            .set_directory("~/")
                            .pick_folder()
                            .await;
                        if let Some(path) = dialog {
                            return Ok(Some(path.path().to_string_lossy().to_string()));
                        }
                        Ok(None::<String>)
                    });
                    self.addon_dir_dialog.set(promise);
                }
                ui.horizontal_wrapped(|ui| {
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
            });
            ui.add_space(5.0);
            ui.separator();
            ui.add_space(5.0);

            // ui.checkbox(
            //     // TODO: make this mean something, currently has no effect
            //     &mut service.config.update_on_launch,
            //     "Check for updates on launch",
            // );
            ui.label(RichText::new("Updates").heading());
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut service.config.update_ttc_pricetable,
                    "Update TTC PriceTable on launch",
                );
                ui.label("(requires TamrielTradeCentre to be installed)");
            });
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut service.config.update_hm_data,
                    "Update HarvestMap data on launch",
                );
                ui.label("(requires HarvestMap-Data to be installed)");
            });
            ui.add_space(5.0);
            ui.separator();
            ui.add_space(5.0);

            ui.label(RichText::new("Import from Minion").heading());
            ui.add_space(5.0);
            ui.horizontal_wrapped(|ui| {
                ui.label("To import addons managed by minion, first create a new backup in minion. Locate the backup folder and select the file with a name like '*-addons.txt'.");
            });
            // TODO: add wiki page to github repo with info on how to backup, how to find backup folder
            // ui.label("For additional help, check this link.");
            ui.add_space(5.0);
            if self.minion_import.is_none() {
                if self.minion_dialog.is_polling() {
                    ui.add_enabled(
                        false,
                        Button::new(RichText::new("Import from Minion...").heading()),
                    );
                } else if ui
                    .button(RichText::new("Import from Minion...").heading())
                    .clicked()
                {
                    let promise = ImmediateValuePromise::new(async move {
                        let dialog = AsyncFileDialog::new()
                            .add_filter("text", &["txt"])
                            .set_directory("~/")
                            .pick_file()
                            .await;
                        if let Some(path) = dialog {
                            return Ok(Some(path.path().to_string_lossy().to_string()));
                        }
                        Ok(None::<String>)
                    });
                    self.minion_dialog.set(promise);
                }
            } else {
                // disabled import progress
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.add_enabled(
                        false,
                        egui::Button::new(RichText::new("Importing...").heading()),
                    );
                });
            }

            ui.add_space(5.0);
            ui.separator();
            ui.add_space(5.0);

            ui.label(RichText::new("Troubleshooting").heading());
            ui.add_space(5.0);
            if let Some(repo) = REPO {
                ui.hyperlink_to("Report an issue", format!("{repo}/issues"));
            }
            // log button to open log output window
            ui.add_space(5.0);
            ui.separator();
            ui.add_space(5.0);

            ui.label(RichText::new("About").heading());
            ui.add_space(5.0);
            ui.label(format!("Version: {}", VERSION));
            if let Some(repo) = REPO {
                ui.hyperlink_to(" GitHub", repo);
            }
        });

        response
    }
}
