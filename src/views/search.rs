use std::collections::HashMap;

use super::{
    ui_helpers::{AddonResponse, AddonTable, PromisedValue, Sort},
    ResetView, View,
};
use eframe::egui;
use eso_addons_core::service::{
    result::{AddonShowDetails, CategoryResult},
    AddonService,
};
use strum::IntoEnumIterator;
use tracing::log::info;

#[derive(Default)]
pub struct Search {
    results: PromisedValue<Vec<AddonShowDetails>>,
    search: String,
    is_init: bool,
    get_categories: PromisedValue<Vec<CategoryResult>>,
    categories: HashMap<i32, CategoryResult>,
    category_addons: PromisedValue<Vec<AddonShowDetails>>,
    displayed_addons: Vec<AddonShowDetails>,
    selected_category: i32,
    previous_category: i32,
    sort: Sort,
    prev_sort: Sort,
}

impl Search {
    pub fn new() -> Self {
        Self {
            sort: Sort::TotalDownloads,
            prev_sort: Sort::Id,
            ..Default::default()
        }
    }
    fn handle_init(&mut self, service: &AddonService) {
        if !self.is_init {
            self.get_categories.set(service.get_categories());
            self.is_init = true;
            self.selected_category = 0;
            self.previous_category = 0;
            self.get_addons(service);
        }
    }

    fn poll(&mut self) {
        self.get_categories.poll();
        if self.get_categories.is_ready() {
            self.get_categories.handle();
            self.categories.clear();
            for category in self.get_categories.value.as_ref().unwrap().iter() {
                self.categories.insert(category.id, category.to_owned());
            }
        }

        self.category_addons.poll();
        if self.category_addons.is_ready() {
            self.category_addons.handle();
            self.sort_addons();
        }

        self.results.poll();
        if self.results.is_ready() {
            self.results.handle();
        }
    }

    fn get_addons(&mut self, service: &AddonService) {
        self.category_addons
            .set(service.get_addons_by_category(self.selected_category));
    }

    fn get_cagetory_title(&self, category_id: i32) -> String {
        self.categories.get(&category_id).unwrap().title.to_owned()
    }

    #[deprecated(since = "0.4.8", note = "please use `reset()` instead")]
    pub fn handle_search(&mut self, service: &mut AddonService) {
        let search_val = self.search.trim().to_lowercase();
        if search_val.is_empty() || self.results.is_polling() {
            return;
        }
        info!("Searching for: {}", search_val);
        self.results.set(service.search(search_val));
    }

    fn handle_sort(&mut self) {
        if self.prev_sort != self.sort {
            self.prev_sort = self.sort;
            self.sort_addons();
        }
    }
    fn sort_addons(&mut self) {
        if self.category_addons.value.as_ref().is_some() {
            self.displayed_addons = self.category_addons.value.as_ref().unwrap().to_vec();
        }
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
}
impl View for Search {
    fn ui(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        service: &mut AddonService,
    ) -> AddonResponse {
        let mut response = AddonResponse::default();
        self.handle_init(service);
        self.poll();

        if self.get_categories.is_polling() {
            ui.spinner();
            return response;
        }

        egui::TopBottomPanel::top("search_top").show(ctx, |ui| {
            self.handle_sort();
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_source("search_category")
                    .selected_text(self.get_cagetory_title(self.selected_category))
                    .show_ui(ui, |ui| {
                        ui.style_mut().wrap = Some(false);
                        for category in self.get_categories.value.as_ref().unwrap() {
                            ui.selectable_value(
                                &mut self.selected_category,
                                category.id,
                                category.title.to_string(),
                            );
                        }
                    });
                egui::ComboBox::from_id_source("search_sort")
                    .selected_text(format!("Sort By: {}", self.sort.to_string().to_uppercase()))
                    .show_ui(ui, |ui| {
                        ui.style_mut().wrap = Some(false);
                        ui.set_min_width(60.0);
                        for sort in Sort::iter() {
                            ui.selectable_value(&mut self.sort, sort, sort.to_string());
                        }
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        // .desired_width(120.0)
                        .hint_text("Search ..."),
                );
                if !self.search.is_empty() && ui.button("🗙").clicked() {
                    self.search.clear();
                    if self.results.value.is_some() {
                        self.results.value.as_mut().unwrap().clear();
                    }
                }
            });
            ui.add_space(5.0);
        });

        if self.selected_category != self.previous_category {
            self.get_addons(service);
            self.previous_category = self.selected_category;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.category_addons.is_polling() {
                ui.spinner();
                return;
            }
            let addons: Vec<&AddonShowDetails> = self
                .displayed_addons
                .iter()
                .filter(|x| {
                    x.name
                        .to_lowercase()
                        .contains(self.search.to_lowercase().as_str())
                })
                .collect();
            ui.centered_and_justified(|ui| {
                response = AddonTable::new(&addons).installable(true).ui(ui);
            });
        });
        response
    }
}
impl ResetView for Search {
    fn reset(&mut self, service: &mut AddonService) {
        if self.is_init {
            self.get_addons(service);
        }
    }
}
