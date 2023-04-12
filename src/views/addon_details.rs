use super::{
    ui_helpers::{ui_show_addon_item, ui_show_bbtree, PromisedValue},
    View,
};
use bbcode_tagger::BBTree;
use eframe::egui::{self, ScrollArea};
use eso_addons_core::service::{result::AddonShowDetails, AddonService};
use tracing::log::info;

#[derive(Default)]
pub struct Details {
    addon_id: i32,
    details: PromisedValue<Option<AddonShowDetails>>,
    parsed_description: PromisedValue<BBTree>,
    parsed_changelog: PromisedValue<BBTree>,
    show_changelog: bool,
}

impl Details {
    fn poll(&mut self, service: &mut AddonService) {
        self.details.poll();
        if self.details.is_ready() {
            self.details.handle();

            if let Some(details) = self.details.value.as_ref().unwrap() {
                // we have details, no setup parse for details and changelog if present
                if let Some(description) = details.description.as_ref() {
                    self.parsed_description
                        .set(service.parse_bbcode(description.to_string()));
                }
                if let Some(changelog) = details.change_log.as_ref() {
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
        info!("Getting addon details for id: {}", addon_id);
        self.details.set(service.get_addon_details(addon_id));
        // if we get a new addon, reset view to description
        self.show_changelog = false;
    }
}
impl View for Details {
    fn ui(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        service: &mut AddonService,
    ) -> Option<i32> {
        self.poll(service);

        if self.details.is_polling() {
            ui.spinner();
            return None;
        }

        if self.details.value.as_ref().unwrap().is_none() {
            ui.label("No addon!");
            return None;
        }

        let addon = self.details.value.as_ref().unwrap().as_ref().unwrap();
        ui.horizontal(|ui| {
            ui_show_addon_item(ui, addon);
            if addon.is_upgradable() {
                // TODO: implement
                ui.button("Update");
            }
            if !addon.installed {
                // TODO: implement
                ui.button("Install");
            }
        });
        ui.separator();

        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.show_changelog, false, "Details");
            ui.selectable_value(&mut self.show_changelog, true, "Change Log");
        });
        ui.separator();

        // ui.vertical_centered_justified(|ui| {
        ScrollArea::vertical().show(ui, |ui| {
            // TODO: add BBCode parsing for description and changelog text
            if !self.show_changelog {
                // show details
                if self.parsed_description.is_ready() {
                    ui_show_bbtree(ui, self.parsed_description.value.as_ref().unwrap());
                } else {
                    ui.label(addon.description.as_ref().unwrap_or(&"".to_string()));
                }
            } else if self.parsed_changelog.is_ready() {
                ui_show_bbtree(ui, self.parsed_changelog.value.as_ref().unwrap());
            } else {
                ui.label(addon.change_log.as_ref().unwrap_or(&"".to_string()));
            }
        });
        // });
        None
    }
}
