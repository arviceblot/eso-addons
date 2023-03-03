use std::collections::HashMap;

use super::{
    ui_helpers::{ui_show_addon_item, PromisedValue},
    View,
};
use eframe::egui::{self, ScrollArea};
use eso_addons_core::service::{result::AddonShowDetails, AddonService};
use tracing::log::info;

#[derive(Default)]
pub struct Search {
    results: PromisedValue<Vec<AddonShowDetails>>,
    install_one: HashMap<i32, PromisedValue<()>>,
    search: String,
}

impl Search {
    fn poll(&mut self, service: &mut AddonService) {
        self.results.poll();
        if self.results.is_ready() {
            self.results.handle();
        }

        let mut installed_addons = vec![];
        for (addon_id, promise) in self.install_one.iter_mut() {
            promise.poll();
            if promise.is_ready() {
                installed_addons.push(addon_id.to_owned());
                promise.handle();
            }
        }
        let fetch_addons = !installed_addons.is_empty();
        for addon_id in installed_addons.iter() {
            self.install_one.remove(addon_id);
        }
        if fetch_addons {
            self.handle_search(service);
        }
    }
    pub fn handle_search(&mut self, service: &mut AddonService) {
        let search_val = self.search.trim().to_lowercase();
        if search_val.is_empty() || self.results.is_polling() {
            return;
        }
        info!("Searching for: {}", search_val);
        self.results.set(service.search(search_val));
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
}
impl View for Search {
    fn ui(
        &mut self,
        _ctx: &egui::Context,
        ui: &mut egui::Ui,
        service: &mut AddonService,
    ) -> Option<i32> {
        self.poll(service);

        ui.horizontal(|ui| {
            let response = ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search"));
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.handle_search(service);
            }
            if ui.button("Search").clicked() {
                self.handle_search(service);
            }
            if ui.button("Clear").clicked() {
                self.search.clear();
                if self.results.value.is_some() {
                    self.results.value.as_mut().unwrap().clear();
                }
            }
        });
        ui.separator();

        if self.results.is_polling() {
            ui.spinner();
            return None;
        } else if self.results.value.is_some() {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Results: {}",
                    self.results.value.as_ref().unwrap().len()
                ));
            });
        }

        if self.results.value.is_none() {
            return None;
        }

        let mut addon_id = None;
        ui.vertical_centered_justified(|ui| {
            ScrollArea::vertical().show(ui, |ui| {
                ui.vertical(|ui| {
                    egui::Grid::new("addon_grid")
                        .striped(true)
                        .spacing([5.0, 20.0])
                        .show(ui, |ui| {
                            let results = self.results.value.as_ref().unwrap().to_owned();
                            // only show not-installed addons in search results
                            for addon in results.iter().filter(|x| !x.installed) {
                                let selected = ui_show_addon_item(ui, addon);
                                if selected.is_some() {
                                    addon_id = selected;
                                }
                                // ui.horizontal_centered(|ui| {
                                if self.is_installing_addon(addon.id) {
                                    ui.add_enabled(false, egui::Button::new("Installing..."));
                                } else if ui.button("Install").clicked() {
                                    self.install_addon(addon.id, service);
                                }
                                // });
                                ui.end_row();
                            }
                        });
                });
            });
        });
        addon_id
    }
}
