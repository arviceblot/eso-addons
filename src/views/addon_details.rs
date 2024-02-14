use std::collections::HashMap;

use super::{
    ui_helpers::{
        truncate, ui_show_addon_item, ui_show_bbtree, ui_show_star, AddonResponse,
        AddonResponseType, PromisedValue,
    },
    View,
};
use bbcode_tagger::BBTree;
use eframe::egui::{self, Layout, RichText, ScrollArea};
use egui_extras::{Column, TableBuilder};
use eso_addons_core::service::{result::AddonShowDetails, AddonService};
use tracing::log::info;

#[derive(Default)]
pub struct Details {
    addon_id: i32,
    details: PromisedValue<Option<AddonShowDetails>>,
    parsed_description: PromisedValue<BBTree>,
    parsed_changelog: PromisedValue<BBTree>,
    show_changelog: bool,
    show_raw_text: bool,
    install_one: HashMap<i32, PromisedValue<()>>,
    update_one: HashMap<i32, PromisedValue<()>>,
}

impl Details {
    fn poll(&mut self, service: &mut AddonService) {
        self.details.poll();
        if self.details.is_ready() {
            self.details.handle();

            if let Some(details) = self.details.value.as_ref().unwrap() {
                // we have details, no setup parse for details and changelog if present
                if let Some(description) = details.description.as_ref() {
                    info!("Parsing BBCode for addon description: {}", details.id);
                    self.parsed_description
                        .set(service.parse_bbcode(description.to_string()));
                }
                if let Some(changelog) = details.change_log.as_ref() {
                    info!("Parsing BBCode for addon changelog: {}", details.id);
                    self.parsed_changelog
                        .set(service.parse_bbcode(changelog.to_string()));
                }
            }
        }

        let mut installed_addons = vec![];
        for (addon_id, promise) in self.install_one.iter_mut() {
            promise.poll();
            if promise.is_ready() {
                installed_addons.push(addon_id.to_owned());
                promise.handle();
            }
        }
        for addon_id in installed_addons.iter() {
            self.install_one.remove(addon_id);
            self.set_addon(*addon_id, service);
        }

        let mut updated_addons = vec![];
        for (addon_id, promise) in self.update_one.iter_mut() {
            promise.poll();
            if promise.is_ready() {
                updated_addons.push(addon_id.to_owned());
                promise.handle();
            }
        }
        for addon_id in updated_addons.iter() {
            self.update_one.remove(addon_id);
            self.set_addon(*addon_id, service);
        }

        self.parsed_description.poll();
        self.parsed_changelog.poll();
    }
    pub fn set_addon(&mut self, addon_id: i32, service: &mut AddonService) {
        self.addon_id = addon_id;
        // get addon details from service
        self.details.set(service.get_addon_details(addon_id));
        // if we get a new addon, reset view to description
        self.show_changelog = false;
    }
    fn install_addon(&mut self, addon_id: i32, service: &mut AddonService) {
        let mut promise = PromisedValue::<()>::default();
        promise.set(service.install(addon_id, true));
        self.install_one.insert(addon_id, promise);
    }
    fn is_installing_addon(&self, addon_id: i32) -> bool {
        let promise = self.install_one.get(&addon_id);
        if promise.is_some() && !promise.unwrap().is_ready() {
            return true;
        }
        false
    }
    fn update_addon(&mut self, addon_id: i32, service: &mut AddonService) {
        let mut promise = PromisedValue::<()>::default();
        promise.set(service.install(addon_id, true));
        self.update_one.insert(addon_id, promise);
    }
    fn is_updating_addon(&self, addon_id: i32) -> bool {
        let promise = self.update_one.get(&addon_id);
        if promise.is_some() && !promise.unwrap().is_ready() {
            return true;
        }
        false
    }
}
impl View for Details {
    fn ui(
        &mut self,
        _ctx: &egui::Context,
        ui: &mut egui::Ui,
        service: &mut AddonService,
    ) -> AddonResponse {
        let mut response = AddonResponse::default();
        self.poll(service);

        if self.details.is_polling() {
            ui.spinner();
            return response;
        }

        if self.details.value.as_ref().unwrap().is_none() {
            ui.label("No addon!");
            return response;
        }

        let addon = self
            .details
            .value
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .to_owned();
        // ui.horizontal(|ui| {
        //     ui_show_addon_item(ui, &addon);
        // });
        ui.horizontal(|ui| {
            //close button
            if ui.button(RichText::new("🗙 Close").heading()).clicked() {
                response.response_type = AddonResponseType::Close;
            }

            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if addon.is_upgradable() {
                    if self.is_updating_addon(addon.id) {
                        ui.add_enabled(false, egui::Button::new("Updating..."));
                    } else if ui.button(RichText::new("Update").heading()).clicked() {
                        self.update_addon(addon.id, service);
                    }
                }
                if !addon.installed {
                    if self.is_installing_addon(addon.id) {
                        ui.add_enabled(false, egui::Button::new("Installing..."));
                    } else if ui.button(RichText::new("Install").heading()).clicked() {
                        self.install_addon(addon.id, service);
                    }
                }
            });
        });
        ui.add_space(5.0);

        ui.horizontal(|ui| {
            // ui.horizontal(|ui| {
            ui.label(RichText::new(addon.name.as_str()).heading().strong());
            if addon
                .download_total
                .as_ref()
                .unwrap()
                .parse::<i32>()
                .unwrap()
                > 5000
            {
                ui_show_star(ui);
            }
            // });
        });
        ui.add_space(5.0);

        ui.horizontal(|ui| {
            ui.selectable_label(
                false,
                RichText::new(format!("by: {}", addon.author_name.as_str())),
            );
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("Version: {}", addon.version));
            });
        });
        // table of values
        // URL
        // ui.label(RichText::new(addon.category.as_str()));
        // compatibility
        // updated
        // created
        // monthly downloads
        // total downloads
        // favorites
        // MD5
        ui.horizontal(|ui| {
            egui::Grid::new("detail_grid")
                .num_columns(2)
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label("URL");
                    ui.hyperlink_to(truncate(&addon.file_info_url), addon.file_info_url);
                    ui.end_row();

                    ui.label("Category:");
                    ui.label(addon.category);
                    ui.end_row();

                    // TODO: pending API client and DB update
                    // ui.label("Compatibility:");
                    // ui.label("");
                    // ui.end_row();

                    ui.label("Updated:");
                    ui.label(addon.date);
                    ui.end_row();

                    // I don't think we have this in the public API?
                    // ui.label("Created:");
                    // ui.label("");
                    // ui.end_row();

                    ui.label("Monthly Downloads:");
                    ui.label(addon.download_monthly.unwrap_or("".to_string()));
                    ui.end_row();

                    ui.label("Total Downloads:");
                    ui.label(addon.download_total.unwrap_or("".to_string()));
                    ui.end_row();

                    ui.label("Favorites:");
                    ui.label(addon.favorite_total.unwrap_or("".to_string()));
                    ui.end_row();

                    ui.label("MD5:");
                    // TODO: add click to copy
                    ui.code(addon.md5.unwrap_or("".to_string()));
                    ui.end_row();
                });
        });
        ui.separator();

        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.show_changelog, false, "Details");
            ui.selectable_value(&mut self.show_changelog, true, "Change Log");
            ui.checkbox(&mut self.show_raw_text, "Show Unformatted Text");
        });
        ui.separator();

        ScrollArea::vertical().show(ui, |ui| {
            if !self.show_changelog {
                // show details
                if self.parsed_description.is_ready() && !self.show_raw_text {
                    ui_show_bbtree(ui, self.parsed_description.value.as_ref().unwrap());
                } else {
                    ui.label(addon.description.as_ref().unwrap_or(&"".to_string()));
                }
            } else if self.parsed_changelog.is_ready() && !self.show_raw_text {
                ui_show_bbtree(ui, self.parsed_changelog.value.as_ref().unwrap());
            } else {
                ui.label(addon.change_log.as_ref().unwrap_or(&"".to_string()));
            }
        });
        response
    }
}
