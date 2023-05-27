use eframe::egui;
use eso_addons_core::service::result::{
    AddonDepOption, AddonMap, AddonShowDetails, MissingDepView,
};
use eso_addons_core::service::AddonService;
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use super::ui_helpers::PromisedValue;
use super::View;

#[derive(Default)]
pub struct MissingDeps {
    missing_deps: HashMap<String, MissingDepView>,
    installed_addons: PromisedValue<Vec<AddonShowDetails>>,
    addon_names: Vec<String>,
    addon_map: AddonMap,
    rev_addon_map: HashMap<String, i32>,
    init: bool,
    install_new: PromisedValue<()>,
}
impl MissingDeps {
    pub fn new() -> Self {
        Self {
            init: true,
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
        self.installed_addons.poll();
        if self.installed_addons.is_ready() {
            self.installed_addons.handle();
            for addon in self.installed_addons.value.as_ref().unwrap().iter() {
                self.addon_map.insert(addon.id, addon.name.clone());
                self.addon_names.push(addon.name.clone());
            }
            self.addon_names.sort_by_key(|a| a.to_lowercase());
            for (id, name) in self.addon_map.iter() {
                self.rev_addon_map.insert(name.clone(), *id);
            }
        }

        // poll installing addons
        self.install_new.poll();
        if self.install_new.is_ready() {
            self.install_new.handle();
            self.missing_deps.clear();
        }
    }
    fn get_installed_addons(&mut self, service: &mut AddonService) {
        if self.installed_addons.is_polling() {
            return;
        }
        self.installed_addons.set(service.get_installed_addons());
    }
    fn install_missing_ready(&self) -> bool {
        self.missing_deps
            .values()
            .all(|x| x.ignore || x.satisfied_by.is_some())
    }

    pub fn has_missing(&self) -> bool {
        !self.missing_deps.is_empty()
    }
    pub fn set_deps(&mut self, deps: Vec<AddonDepOption>) {
        for dep in deps.iter() {
            let dep_view = match self.missing_deps.entry(dep.missing_dir.clone()) {
                Entry::Occupied(o) => o.into_mut(),
                Entry::Vacant(v) => {
                    let missing_dep = MissingDepView::new(dep.required_by.clone());
                    v.insert(missing_dep)
                }
            };
            dep_view.missing_dir = dep.missing_dir.clone();
            if let Some(option_id) = dep.option_id {
                dep_view
                    .options
                    .insert(option_id, dep.option_name.as_ref().unwrap().clone());
            }
        }
    }
    fn install_new(&mut self, service: &mut AddonService) {
        // Install selected missing dep addons or set to ignore
        let vecs: Vec<MissingDepView> = self.missing_deps.values().cloned().collect();
        self.install_new
            .set(service.install_missing_dependencies(vecs));
    }
}

impl View for MissingDeps {
    fn ui(
        &mut self,
        ctx: &eframe::egui::Context,
        ui: &mut eframe::egui::Ui,
        service: &mut eso_addons_core::service::AddonService,
    ) -> Option<i32> {
        // get installed addons on init
        if self.show_init() {
            self.get_installed_addons(service);
        }

        // check poll for getting installed addons
        self.poll(service);

        // if installing addons, don't show everything else
        if self.install_new.is_polling() {
            ui.spinner();
            return None;
        }

        // show missing deps when ready
        egui::TopBottomPanel::top("top_panel")
            .show_inside(ui, |ui| {
                ui.heading("Missing Dependencies");
                ui.label("Some installed addons have missing dependencies. Please select whether the missing dependency should be ignored, is already satisfied by an existing addon, or install one of the suggested addons.");
            });

        egui::TopBottomPanel::bottom("bottom_panel").show_inside(ui, |ui| {
            ui.add_enabled_ui(self.install_missing_ready(), |ui| {
                if ui.button("Install").clicked() {
                    self.install_new(service);
                }
            });
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (missing_dir, dep_opt) in self.missing_deps.iter_mut() {
                    ui.horizontal(|ui| {
                        ui.strong(missing_dir);
                        ui.label(format!("Required By: {}", dep_opt.required_by));
                    });
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut dep_opt.ignore, "Ignore");
                        ui.add_enabled_ui(!dep_opt.ignore, |ui| {
                            // select from installed addons
                            egui::ComboBox::from_id_source(format!("satisfied_by_{}", missing_dir))
                                .selected_text(
                                    self.addon_map
                                        .get(&dep_opt.satisfied_by.unwrap_or(0))
                                        .unwrap_or(&String::new())
                                        .as_str(),
                                )
                                .width(200.0)
                                .show_ui(ui, |ui| {
                                    for name in self.addon_names.iter() {
                                        let mut val = 0;
                                        let id = self.rev_addon_map.get(name).unwrap_or(&0);
                                        ui.selectable_value(&mut val, *id, name);
                                        if val != 0 {
                                            dep_opt.satisfied_by = Some(val);
                                        }
                                    }
                                });
                            if !dep_opt.options.is_empty() {
                                // select from suggested addon
                                egui::ComboBox::from_id_source(format!("opt_by_{}", missing_dir))
                                    .selected_text(
                                        dep_opt
                                            .options
                                            .get(&dep_opt.satisfied_by.unwrap_or(0))
                                            .unwrap_or(&String::new())
                                            .as_str(),
                                    )
                                    .width(200.0)
                                    .show_ui(ui, |ui| {
                                        for (opt_id, opt_name) in dep_opt.options.iter() {
                                            let mut val = 0;
                                            ui.selectable_value(&mut val, *opt_id, opt_name);
                                            if val != 0 {
                                                dep_opt.satisfied_by = Some(val);
                                            }
                                        }
                                    });
                            }
                        });
                    });
                    ui.separator();
                }
            });
        });
        None
    }
}
