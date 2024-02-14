use std::collections::HashMap;
use tracing::log::info;

use eframe::{
    egui::{self, Layout, RichText, ScrollArea},
    epaint::Color32,
};
use eso_addons_core::service::{
    result::{AddonShowDetails, UpdateResult},
    AddonService,
};
use strum::IntoEnumIterator;

use super::{
    ui_helpers::{
        ui_show_addon_item, AddonResponse, AddonResponseType, AddonTable, PromisedValue, Sort,
    },
    View,
};

#[derive(Default)]
pub struct Installed {
    // addons_promise: Option<ImmediateValuePromise<Vec<AddonShowDetails>>>,
    installed_addons: PromisedValue<Vec<AddonShowDetails>>,
    update_one: HashMap<i32, PromisedValue<()>>,
    pub update: PromisedValue<UpdateResult>,
    remove: PromisedValue<()>,
    ttc_pricetable: PromisedValue<()>,
    hm_data: PromisedValue<()>,
    displayed_addons: Vec<AddonShowDetails>,
    log: Vec<String>,
    filter: String,
    sort: Sort,
    prev_sort: Sort,
    init: bool,
    editing: bool,
}

impl Installed {
    pub fn new() -> Installed {
        Installed {
            installed_addons: PromisedValue::default(),
            update: PromisedValue::default(),
            remove: PromisedValue::default(),
            ttc_pricetable: PromisedValue::default(),
            update_one: HashMap::new(),
            displayed_addons: vec![],
            log: vec![],
            filter: Default::default(),
            sort: Sort::Name,
            prev_sort: Sort::Id,
            init: true,
            editing: false,
            ..Default::default()
        }
    }
    fn show_init(&mut self) -> bool {
        let init = self.init;
        if self.init {
            self.init = false;
        }
        init
    }
    fn poll(&mut self, service: &mut AddonService) {
        self.update.poll();
        if self.update.is_ready() && !self.installed_addons.is_polling() {
            self.update.handle();
            self.log.push("Updated addon list.".to_string());
            self.get_installed_addons(service);
        }
        self.ttc_pricetable.poll();
        if self.ttc_pricetable.is_ready() {
            self.log.push("Updated TTC PriceTable.".to_string());
            self.ttc_pricetable.handle();
        }
        self.hm_data.poll();
        if self.hm_data.is_ready() {
            self.log.push("Updated HarvestMap data.".to_string());
            self.hm_data.handle();
        }

        self.installed_addons.poll();
        if self.installed_addons.is_ready() {
            self.installed_addons.handle();
            // force sort as addons list may have updated
            self.sort_addons();
        }

        self.remove.poll();
        if self.remove.is_ready() {
            self.remove.handle();
            self.get_installed_addons(service);
        }

        let mut updated_addons = vec![];
        for (addon_id, promise) in self.update_one.iter_mut() {
            promise.poll();
            if promise.is_ready() {
                updated_addons.push(addon_id.to_owned());
                promise.handle();
                self.log.push(format!("Updated addon: {addon_id}"));
            }
        }
        let fetch_addons = !updated_addons.is_empty();
        for addon_id in updated_addons.iter() {
            self.update_one.remove(addon_id);
        }
        if fetch_addons {
            self.get_installed_addons(service);
        }
    }
    fn is_updating_addon(&self, addon_id: i32) -> bool {
        let promise = self.update_one.get(&addon_id);
        if promise.is_some() && !promise.unwrap().is_ready() {
            return true;
        }
        false
    }
    fn update_addons(&mut self, service: &mut AddonService) {
        if self.installed_addons.is_polling() {
            return;
        }
        let update_ids = self
            .installed_addons
            .value
            .as_ref()
            .unwrap()
            .iter()
            .filter(|x| x.is_upgradable())
            .map(|x| x.id);
        for update_id in update_ids {
            let mut promise = PromisedValue::<()>::default();
            promise.set(service.install(update_id, true));
            self.update_one.insert(update_id, promise);
        }
    }
    pub fn get_installed_addons(&mut self, service: &mut AddonService) {
        if self.installed_addons.is_polling() {
            return;
        }
        info!("Getting installed addons");
        self.installed_addons.set(service.get_installed_addons());
    }

    fn check_update(&mut self, service: &mut AddonService) {
        // Check for updates but do not upgrade any addons
        info!("Checking for updates");
        self.update.set(service.update(false));
        // check update TTC PriceTable
        if service.config.update_ttc_pricetable {
            self.ttc_pricetable.set(service.update_ttc_pricetable());
        }
        // check HarvestMap data
        if service.config.update_hm_data {
            self.hm_data.set(service.update_hm_data());
        }
    }
    fn handle_sort(&mut self) {
        if self.prev_sort != self.sort {
            self.prev_sort = self.sort;
            self.sort_addons();
        }
    }
    fn sort_addons(&mut self) {
        if self.installed_addons.value.as_ref().is_some() {
            self.displayed_addons = self.installed_addons.value.as_ref().unwrap().to_vec();
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

    fn remove_addon(&mut self, addon_id: i32, service: &mut AddonService) {
        let mut promise = PromisedValue::<()>::default();
        promise.set(service.remove(addon_id));
        self.remove = promise;
    }

    fn update_addon(&mut self, addon_id: i32, service: &mut AddonService) {
        let mut promise = PromisedValue::<()>::default();
        promise.set(service.install(addon_id, true));
        self.update_one.insert(addon_id, promise);
    }

    fn get_updateable_addon_count(&self) -> usize {
        self.installed_addons
            .value
            .as_ref()
            .unwrap()
            .iter()
            .filter(|x| x.is_upgradable())
            .count()
    }
}
impl View for Installed {
    fn ui(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        service: &mut AddonService,
    ) -> AddonResponse {
        let mut response = AddonResponse::default();
        if self.show_init() {
            self.check_update(service);
        }

        // update promises
        self.poll(service);

        // if we are loading addons, show spinner and that's it
        if self.installed_addons.is_polling() || self.update.is_polling() {
            ui.spinner();
            return response;
        }

        egui::TopBottomPanel::bottom("installed_bottom").show(ctx, |ui| {
            // log scroll area
            egui::CollapsingHeader::new("Log")
                .default_open(true)
                .show(ui, |ui| {
                    ui.vertical_centered_justified(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ScrollArea::vertical()
                            .max_height(20.0)
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    for update in self.log.iter() {
                                        ui.label(update);
                                    }
                                });
                            });
                    })
                });
        });

        if !self.installed_addons.is_polling()
            && self.installed_addons.value.as_ref().unwrap().is_empty()
        {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("No addons installed!");
            });
        } else {
            self.handle_sort();
            egui::TopBottomPanel::top("installed_top").show(ctx, |ui| {
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    let updateable_count = self.get_updateable_addon_count();
                    ui.label(
                        RichText::new(format!(
                            "Installed - {} addons ({})",
                            self.installed_addons.value.as_ref().unwrap().len(),
                            updateable_count
                        ))
                        .heading()
                        .strong(),
                    );
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if !self.update_one.is_empty() {
                            ui.add_enabled(false, egui::Button::new("Updating..."));
                        } else if updateable_count > 0
                            && ui
                                .button(RichText::new("⮉ Update All").heading().strong())
                                .clicked()
                        {
                            self.update_addons(service);
                        }
                        if ui
                            .button(RichText::new("🔄 Check for Updates").heading().strong())
                            .clicked()
                        {
                            self.check_update(service);
                        }
                        ui.checkbox(&mut self.editing, "Edit");
                    });
                });
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_source("sort")
                        .selected_text(format!("Sort By: {}", self.sort.to_string().to_uppercase()))
                        .show_ui(ui, |ui| {
                            ui.style_mut().wrap = Some(false);
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
                response = AddonTable::new(&addons).installable(true).ui(ui);
            });
        }

        response
    }
}
