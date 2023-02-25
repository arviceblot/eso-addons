use eframe::{
    egui::{self, RichText, ScrollArea},
    epaint::Color32,
};
use eso_addons_core::service::{result::AddonShowDetails, AddonService};
use strum::IntoEnumIterator;
use tokio::runtime::Runtime;

use super::{
    ui_helpers::{ui_show_addon_item, Sort},
    View,
};

pub struct Installed {
    installed_addons: Vec<AddonShowDetails>,
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
            installed_addons: vec![],
            displayed_addons: vec![],
            addons_updated: vec![],
            filter: Default::default(),
            sort: Sort::Name,
            prev_sort: Sort::Name,
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
    fn update_addons(&mut self, rt: &Runtime, service: &mut AddonService) {
        let result = rt.block_on(service.upgrade()).unwrap();
        for update in result.addons_updated.iter() {
            self.addons_updated
                .push(format!("{} updated!", update.name));
        }
        if result.addons_updated.is_empty() {
            self.addons_updated
                .push("Everything up to date!".to_string());
        }

        if service.config.update_ttc_pricetable.unwrap_or(false) {
            rt.block_on(service.update_ttc_pricetable()).unwrap();
            self.addons_updated
                .push("TTC PriceTable Updated!".to_string());
        }
        self.get_installed_addons(rt, service);
    }
    pub fn get_installed_addons(&mut self, rt: &Runtime, service: &mut AddonService) {
        let result = rt.block_on(service.get_installed_addons()).unwrap();
        self.installed_addons = result;
        self.sort_addons();
    }
    fn handle_sort(&mut self) {
        if self.prev_sort != self.sort {
            self.prev_sort = self.sort;
            self.sort_addons();
        }
    }
    fn sort_addons(&mut self) {
        self.displayed_addons = self.installed_addons.to_vec();
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

    fn remove_addon(&self, addon_id: i32, rt: &Runtime, service: &mut AddonService) {
        rt.block_on(service.remove(addon_id)).unwrap();
    }
}
impl View for Installed {
    fn ui(
        &mut self,
        _ctx: &egui::Context,
        ui: &mut egui::Ui,
        rt: &Runtime,
        service: &mut AddonService,
    ) {
        if self.show_init() {
            // TODO: move blocking install count out of update loop!
            rt.block_on(service.update(false)).unwrap();
            if service.config.update_on_launch.unwrap_or(false) {
                rt.block_on(service.upgrade()).unwrap();
            }
            self.get_installed_addons(rt, service);
        }

        if self.installed_addons.is_empty() {
            ui.label("No addons installed!");
        } else {
            self.handle_sort();
            ui.horizontal(|ui| {
                if ui.button("Update All").clicked() {
                    // TODO: move blocking update out of update loop!
                    self.update_addons(rt, service);
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
                ui.label(format!("Installed: {}", self.installed_addons.len()));
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
                                            ui.horizontal_centered(|ui| {
                                                if ui
                                                    .button(RichText::new("🗙").color(Color32::RED))
                                                    .clicked()
                                                {
                                                    remove_id = Some(addon.id);
                                                }
                                            });
                                        }
                                        ui_show_addon_item(ui, addon);

                                        if addon.is_upgradable() && ui.button("Update").clicked() {
                                            rt.block_on(service.install(addon.id, true)).unwrap();
                                        }
                                        ui.end_row();
                                    }
                                });
                            if let Some(id) = remove_id {
                                self.remove_addon(id, rt, service);
                                self.get_installed_addons(rt, service);
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
    }
}
