use tracing::log::info;

use eframe::egui::{self, Layout, RichText};
use eso_addons_core::service::{result::AddonShowDetails, AddonService};
use strum::IntoEnumIterator;

use super::{
    ui_helpers::{AddonResponse, AddonResponseType, AddonTable, Sort},
    View,
};

#[derive(Default)]
pub struct Installed {
    displayed_addons: Vec<AddonShowDetails>,
    filter: String,
    sort: Sort,
    prev_sort: Sort,
}

impl Installed {
    pub fn new() -> Installed {
        Installed {
            displayed_addons: vec![],
            filter: Default::default(),
            sort: Sort::Name,
            prev_sort: Sort::Id,
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
            self.prev_sort = self.sort;
            self.sort_addons();
        }
    }
    fn sort_addons(&mut self) {
        info!("Sorting addons");
        match self.sort {
            Sort::Author => self.displayed_addons.sort_unstable_by(|a, b| {
                a.author_name
                    .to_lowercase()
                    .cmp(&b.author_name.to_lowercase())
            }),
            Sort::Name => self
                .displayed_addons
                .sort_unstable_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            Sort::Updated => self
                .displayed_addons
                .sort_unstable_by(|a, b| a.date.cmp(&b.date)),
            Sort::TotalDownloads => self.displayed_addons.sort_unstable_by(|a, b| {
                b.download_total
                    .as_ref()
                    .unwrap_or(&"0".to_string())
                    .parse::<i32>()
                    .unwrap_or(0)
                    .cmp(
                        &a.download_total
                            .as_ref()
                            .unwrap_or(&"0".to_string())
                            .parse::<i32>()
                            .unwrap_or(0),
                    )
            }),
            Sort::MonthlyDownloads => self.displayed_addons.sort_unstable_by(|a, b| {
                b.download
                    .as_ref()
                    .unwrap_or(&"0".to_string())
                    .parse::<i32>()
                    .unwrap_or(0)
                    .cmp(
                        &a.download
                            .as_ref()
                            .unwrap_or(&"0".to_string())
                            .parse::<i32>()
                            .unwrap_or(0),
                    )
            }),
            Sort::Favorites => self.displayed_addons.sort_unstable_by(|a, b| {
                b.favorite_total
                    .as_ref()
                    .unwrap_or(&"0".to_string())
                    .parse::<i32>()
                    .unwrap_or(0)
                    .cmp(
                        &a.favorite_total
                            .as_ref()
                            .unwrap_or(&"0".to_string())
                            .parse::<i32>()
                            .unwrap_or(0),
                    )
            }),
            Sort::Id => self
                .displayed_addons
                .sort_unstable_by(|a, b| a.id.cmp(&b.id)),
        }

        // secondary sort, put upgradeable at top
        self.displayed_addons
            .sort_unstable_by_key(|b| std::cmp::Reverse(b.is_upgradable()));
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
        ctx: &egui::Context,
        _ui: &mut egui::Ui,
        _service: &mut AddonService,
    ) -> AddonResponse {
        let mut response = AddonResponse::default();

        if self.displayed_addons.is_empty() {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.heading("No addons installed!");
                })
            });
        } else {
            self.handle_sort();
            egui::TopBottomPanel::top("installed_top").show(ctx, |ui| {
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
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
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

            egui::CentralPanel::default().show(ctx, |ui| {
                let addons: Vec<&AddonShowDetails> = self
                    .displayed_addons
                    .iter()
                    .filter(|x| {
                        x.name
                            .to_lowercase()
                            .contains(self.filter.to_lowercase().as_str())
                    })
                    .collect();
                ui.centered_and_justified(|ui| {
                    response = AddonTable::new(&addons).installable(true).ui(ui);
                });
            });
        }

        response
    }
}
