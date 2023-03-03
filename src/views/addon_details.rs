use std::collections::HashMap;

use super::{
    ui_helpers::{ui_show_addon_item, PromisedValue},
    View,
};
use eframe::egui::{self, ScrollArea};
use eso_addons_core::service::{result::AddonShowDetails, AddonService};
use tracing::log::info;

#[derive(Default)]
pub struct Details {
    addon_id: i32,
    details: PromisedValue<Option<AddonShowDetails>>,
    show_changelog: bool,
}

impl Details {
    fn poll(&mut self, service: &mut AddonService) {
        self.details.poll();
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
                ui.label(addon.description.as_ref().unwrap_or(&"".to_string()));
            } else {
                ui.label(addon.change_log.as_ref().unwrap_or(&"".to_string()));
            }
        });
        // });
        None
    }
}
