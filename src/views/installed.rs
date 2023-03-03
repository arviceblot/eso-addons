use std::collections::HashMap;
use tracing::log::info;

use eframe::{
    egui::{self, RichText, ScrollArea},
    epaint::Color32,
};
use eso_addons_core::service::{
    result::{AddonShowDetails, UpdateResult},
    AddonService,
};
use strum::IntoEnumIterator;

use super::{
    ui_helpers::{ui_show_addon_item, PromisedValue, Sort},
    View,
};

pub struct Installed {
    // addons_promise: Option<ImmediateValuePromise<Vec<AddonShowDetails>>>,
    installed_addons: PromisedValue<Vec<AddonShowDetails>>,
    update_one: HashMap<i32, PromisedValue<()>>,
    update: PromisedValue<UpdateResult>,
    remove: PromisedValue<()>,
    displayed_addons: Vec<AddonShowDetails>,
    addons_updated: Vec<String>,
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
            update_one: HashMap::new(),
            displayed_addons: vec![],
            addons_updated: vec![],
            filter: Default::default(),
            sort: Sort::Name,
            prev_sort: Sort::Id,
            init: true,
            editing: false,
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
            self.get_installed_addons(service);
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
        if !self.installed_addons.is_ready() {
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
        // let result = rt.block_on(service.upgrade()).unwrap();
        // for update in result.addons_updated.iter() {
        //     self.addons_updated
        //         .push(format!("{} updated!", update.name));
        // }
        // if result.addons_updated.is_empty() {
        //     self.addons_updated
        //         .push("Everything up to date!".to_string());
        // }

        // if service.config.update_ttc_pricetable.unwrap_or(false) {
        //     rt.block_on(service.update_ttc_pricetable()).unwrap();
        //     self.addons_updated
        //         .push("TTC PriceTable Updated!".to_string());
        // }
        // self.get_installed_addons(rt, service);
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
        // rt.block_on(service.remove(addon_id)).unwrap();
    }
}
impl View for Installed {
    fn ui(
        &mut self,
        _ctx: &egui::Context,
        ui: &mut egui::Ui,
        service: &mut AddonService,
    ) -> Option<i32> {
        if self.show_init() {
            self.check_update(service);
        }

        // update promises
        self.poll(service);

        // if we are loading addons, show spinner and that's it
        if self.installed_addons.is_polling() || self.update.is_polling() {
            ui.spinner();
            return None;
        }

        let mut return_id = None;
        if !self.installed_addons.is_polling()
            && self.installed_addons.value.as_ref().unwrap().is_empty()
        {
            ui.label("No addons installed!");
        } else {
            self.handle_sort();
            ui.horizontal(|ui| {
                if !self.update_one.is_empty() {
                    ui.add_enabled(false, egui::Button::new("Updating..."));
                } else if ui.button("Update All").clicked() {
                    self.update_addons(service);
                }
                egui::ComboBox::from_id_source("sort")
                    .selected_text(format!("Sort By: {}", self.sort.to_string().to_uppercase()))
                    .show_ui(ui, |ui| {
                        ui.style_mut().wrap = Some(false);
                        ui.set_min_width(60.0);
                        for sort in Sort::iter() {
                            ui.selectable_value(&mut self.sort, sort, sort.to_string());
                        }
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .desired_width(120.0)
                        .hint_text("Filter..."),
                );
                if ui.button("🗙").clicked() {
                    self.filter.clear();
                }
            });
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Installed: {}",
                    self.installed_addons.value.as_ref().unwrap().len()
                ));
                ui.checkbox(&mut self.editing, "Edit");
            });
            ui.separator();
            ui.vertical_centered_justified(|ui| {
                ScrollArea::vertical()
                    .max_height(300.0)
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            let mut remove_id: Option<i32> = Default::default();
                            egui::Grid::new("addon_grid")
                                .striped(true)
                                .spacing([5.0, 20.0])
                                .show(ui, |ui| {
                                    for addon in self.displayed_addons.iter().filter(|x| {
                                        self.filter.is_empty()
                                            || x.name
                                                .to_lowercase()
                                                .contains(&self.filter.to_lowercase())
                                    }) {
                                        // col0 x button if editing
                                        if self.editing {
                                            if self.remove.is_polling() {
                                                ui.spinner();
                                            } else {
                                                ui.horizontal_centered(|ui| {
                                                    if ui
                                                        .button(
                                                            RichText::new("🗙").color(Color32::RED),
                                                        )
                                                        .clicked()
                                                    {
                                                        remove_id = Some(addon.id);
                                                    }
                                                });
                                            }
                                        }
                                        let addon_id = ui_show_addon_item(ui, addon).to_owned();
                                        if addon_id.is_some() {
                                            return_id = addon_id;
                                        }

                                        if addon.is_upgradable() {
                                            if self.is_updating_addon(addon.id) {
                                                ui.add_enabled(
                                                    false,
                                                    egui::Button::new("Updating..."),
                                                );
                                            } else if ui.button("Update").clicked() {
                                                let mut promise = PromisedValue::<()>::default();
                                                promise.set(service.install(addon.id, true));
                                                self.update_one.insert(addon.id, promise);
                                            }
                                        }
                                        ui.end_row();
                                    }
                                });
                            if let Some(id) = remove_id {
                                self.remove_addon(id, service);
                            }
                        });
                    });
            });
            ui.separator();
        }
        // log scroll area
        ui.collapsing("Log", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ScrollArea::vertical().max_height(20.0).show(ui, |ui| {
                    ui.vertical(|ui| {
                        for update in self.addons_updated.iter() {
                            ui.label(update);
                        }
                    });
                });
            });
        });

        return_id
    }
}
