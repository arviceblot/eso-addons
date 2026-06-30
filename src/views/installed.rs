use tracing::log::info;

use eframe::egui::{self, Layout, RichText, TextWrapMode};
use eso_addons_core::service::{AddonService, result::AddonShowDetails};
use strum::IntoEnumIterator;

use super::{
    View,
    ui_helpers::{AddonResponse, AddonResponseType, AddonTable, Sort},
};

const LIBRARY_CATEGORY: &str = "Libraries";

#[derive(Default)]
pub struct Installed {
    displayed_addons: Vec<AddonShowDetails>,
    filter: String,
    sort: Sort,
    prev_sort: Sort,
    ascending: bool,
    prev_ascending: bool,
}

impl Installed {
    pub fn new() -> Installed {
        Installed {
            displayed_addons: vec![],
            filter: Default::default(),
            sort: Sort::Name,
            prev_sort: Sort::Id,
            ascending: Sort::Name.default_ascending(),
            prev_ascending: Sort::Name.default_ascending(),
        }
    }
    pub fn displayed_addons(mut self, addons: Vec<AddonShowDetails>) -> Self {
        self.displayed_addons = addons.to_vec();
        self.sort_addons();
        self
    }
    fn update_addons(&mut self) -> AddonResponse {
        let mut response = AddonResponse::default();
        let update_ids: Vec<i32> = self
            .displayed_addons
            .iter()
            .filter(|x| x.is_upgradable())
            .map(|x| x.id)
            .collect();
        response.response_type = AddonResponseType::UpdateMultiple;
        response.addon_ids = update_ids;
        response
    }

    fn handle_sort(&mut self) {
        if self.prev_sort != self.sort {
            self.ascending = self.sort.default_ascending();
        } else if self.prev_ascending == self.ascending {
            return;
        }
        self.prev_sort = self.sort;
        self.prev_ascending = self.ascending;
        self.sort_addons();
    }
    fn sort_addons(&mut self) {
        info!("Sorting addons");
        super::ui_helpers::sort_addons(&mut self.displayed_addons, self.sort, self.ascending);
    }

    fn get_updateable_addon_count(&self) -> usize {
        self.displayed_addons
            .iter()
            .filter(|x| x.is_upgradable())
            .count()
    }
}
impl View for Installed {
    fn ui(
        &mut self,
        _ctx: &egui::Context,
        ui: &mut egui::Ui,
        _service: &mut AddonService,
    ) -> AddonResponse {
        let mut response = AddonResponse::default();

        if self.displayed_addons.is_empty() {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.heading("No addons installed!");
                })
            });
        } else {
            self.handle_sort();
            egui::Panel::top("installed_top").show_inside(ui, |ui| {
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    let updateable_count = self.get_updateable_addon_count();
                    ui.label(
                        RichText::new(format!(
                            "Installed - {} addons", // ({})",
                            self.displayed_addons.len(),
                            // updateable_count
                        ))
                        .heading(),
                    );
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        // if !self.update_one.is_empty() {
                        //     ui.add_enabled(false, egui::Button::new("Updating..."));
                        // } else if updateable_count > 0
                        if updateable_count > 0
                            && ui.button(RichText::new("⮉ Update All").heading()).clicked()
                        {
                            response = self.update_addons();
                        }
                        if ui
                            .button(RichText::new("🔄 Check for Updates").heading())
                            .clicked()
                        {
                            response.response_type = AddonResponseType::CheckUpdate;
                        }
                    });
                });
                ui.add_space(5.0);

                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("sort")
                        .selected_text(format!("Sort By: {}", self.sort.to_string().to_uppercase()))
                        .show_ui(ui, |ui| {
                            ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                            ui.set_min_width(60.0);
                            for sort in Sort::iter() {
                                ui.selectable_value(&mut self.sort, sort, sort.to_string());
                            }
                        });
                    ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Search ..."));
                    if ui.button("🗙").clicked() {
                        self.filter.clear();
                    }
                });
                ui.add_space(5.0);
            });
            // shortcut any overriding response before table view (update, etc.)
            if response.response_type != AddonResponseType::default() {
                return response;
            }

            let filter = self.filter.to_lowercase();
            let matched: Vec<&AddonShowDetails> = self
                .displayed_addons
                .iter()
                .filter(|x| x.name.to_lowercase().contains(filter.as_str()))
                .collect();

            // While searching, keep a single unified list so library matches aren't
            // squeezed into the bottom panel.
            if filter.is_empty() {
                let (libraries, addons): (Vec<&AddonShowDetails>, Vec<&AddonShowDetails>) = matched
                    .into_iter()
                    .partition(|x| x.category == LIBRARY_CATEGORY);

                if !libraries.is_empty() {
                    egui::Panel::bottom("installed_libraries")
                        .resizable(true)
                        .default_size(200.0)
                        .show_inside(ui, |ui| {
                            ui.add_space(5.0);
                            ui.heading(format!("Libraries - {} addons", libraries.len()));
                            ui.add_space(5.0);
                            let lib_response = AddonTable::new(&libraries).installable(true).ui(
                                ui,
                                &mut self.sort,
                                &mut self.ascending,
                            );
                            if lib_response.response_type != AddonResponseType::default() {
                                response = lib_response;
                            }
                        });
                }

                egui::CentralPanel::default().show_inside(ui, |ui| {
                    let addon_response = AddonTable::new(&addons).installable(true).ui(
                        ui,
                        &mut self.sort,
                        &mut self.ascending,
                    );
                    if addon_response.response_type != AddonResponseType::default() {
                        response = addon_response;
                    }
                });
            } else {
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    response = AddonTable::new(&matched).installable(true).ui(
                        ui,
                        &mut self.sort,
                        &mut self.ascending,
                    );
                });
            }
        }

        response
    }
}
