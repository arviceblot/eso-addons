use super::{
    ui_helpers::{ui_show_bbtree, ui_show_star, AddonResponse, AddonResponseType, PromisedValue},
    ResetView, View,
};
use bbcode_tagger::BBTree;
use eframe::egui::{self, Layout, RichText, ScrollArea};
use eso_addons_core::service::{result::AddonShowDetails, AddonService};
use tracing::info;

#[derive(Default)]
pub struct Details {
    addon_id: i32,
    details: PromisedValue<Option<AddonShowDetails>>,
    parsed_description: PromisedValue<BBTree>,
    parsed_changelog: PromisedValue<BBTree>,
    show_changelog: bool,
    show_raw_text: bool,
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
            if ui.button(RichText::new("⮪ Close").heading()).clicked() {
                response.response_type = AddonResponseType::Close;
            }

            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if addon.is_upgradable() {
                    // if self.is_updating_addon(addon.id) {
                    //     ui.add_enabled(false, egui::Button::new("Updating..."));
                    // } else if ui.button(RichText::new("⮉ Update").heading()).clicked() {
                    if ui.button(RichText::new("⮉ Update").heading()).clicked() {
                        response.addon_id = addon.id;
                        response.response_type = AddonResponseType::Update;
                    }
                }
                if !addon.installed {
                    // if self.is_installing_addon(addon.id) {
                    //     ui.add_enabled(false, egui::Button::new("Installing..."));
                    // } else if ui.button(RichText::new("⮋ Install").heading()).clicked() {
                    if ui.button(RichText::new("⮋ Install").heading()).clicked() {
                        response.addon_id = addon.id;
                        response.response_type = AddonResponseType::Install
                    }
                } else if ui.button(RichText::new("🗙 Remove").heading()).clicked() {
                    response.response_type = AddonResponseType::Remove;
                    response.addon_id = addon.id;
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
            if ui
                .selectable_label(
                    false,
                    RichText::new(format!("by: {}", addon.author_name.as_str())),
                )
                .clicked()
            {
                response.author_name = addon.author_name;
                response.response_type = AddonResponseType::AuthorName;
            }
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                ui.hyperlink_to("Visit Website", addon.file_info_url);
            });
        });

        ui.horizontal(|ui| {
            // TODO: Add icon
            ui.label(addon.category);
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                // TODO: pretty format date like: January 1, 2024
                ui.label(format!("🕘 Updated {}", addon.date));
            });
        });
        ui.horizontal(|ui| {
            if let Some(compat_version) = addon.game_compat_version {
                ui.label(format!(
                    "⛭ {} ({}) Supported",
                    addon.game_compat_name.unwrap(),
                    compat_version
                ));
            } else {
                ui.label("⛭ Unknown Version Supported");
            }
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                // TODO: pretty print download count
                ui.label(format!(
                    "⮋ {} Downloads",
                    addon.download_total.unwrap_or("".to_string())
                ));
            });
        });
        ui.horizontal(|ui| {
            ui.label(format!("🔁 Version {}", addon.version));
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                // TODO: pretty print favorite count
                ui.label(format!(
                    "♥ {} Favorites",
                    addon.favorite_total.unwrap_or("".to_string())
                ));
            });
        });
        ui.separator();

        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.show_changelog,
                false,
                RichText::new("Details").heading(),
            );
            ui.selectable_value(
                &mut self.show_changelog,
                true,
                RichText::new("Change Log").heading(),
            );
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
impl ResetView for Details {
    fn reset(&mut self, service: &mut AddonService) {
        // do not get if not addon id set yet
        if self.addon_id == i32::default() {
            return;
        }
        // re-get details for same addon
        self.details.set(service.get_addon_details(self.addon_id));
    }
}
