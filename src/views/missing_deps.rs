use eframe::egui::{self, Layout, RichText};
use eso_addons_core::service::result::{AddonDepOption, AddonMap, MissingDepView};
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use super::ui_helpers::{AddonResponse, AddonResponseType};
use super::View;

#[derive(Default)]
pub struct MissingDeps {
    missing_deps: HashMap<String, MissingDepView>,
    addon_names: Vec<String>,
    addon_map: AddonMap,
    rev_addon_map: HashMap<String, i32>,
}
impl MissingDeps {
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }

    fn install_missing_ready(&self) -> bool {
        // ready to respond install is all options are marked as ignored or satisfied
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
    fn install_new(&mut self) -> AddonResponse {
        // Install selected missing dep addons or set to ignore
        let vecs: Vec<MissingDepView> = self.missing_deps.values().cloned().collect();
        AddonResponse {
            missing_deps: vecs,
            response_type: AddonResponseType::InstallMissingDeps,
            ..Default::default()
        }
    }
}

impl View for MissingDeps {
    fn ui(
        &mut self,
        _ctx: &eframe::egui::Context,
        ui: &mut eframe::egui::Ui,
        _service: &mut eso_addons_core::service::AddonService,
    ) -> AddonResponse {
        let mut response = AddonResponse::default();

        // show missing deps when ready
        egui::TopBottomPanel::top("top_panel")
            .show_inside(ui, |ui| {
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Missing Dependencies").heading().strong());
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_enabled_ui(self.install_missing_ready(), |ui| {
                            if ui
                                .button(RichText::new("Install").heading().strong())
                                .clicked()
                            {
                                response = self.install_new();
                            }
                        });
                    });
                });
                            ui.add_space(5.0);
                ui.label("Some installed addons have missing dependencies. Please select whether the missing dependency should be ignored, is already satisfied by an existing addon, or install one of the suggested addons.");
                ui.add_space(5.0);
            });

        if response.response_type != AddonResponseType::default() {
            return response;
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (missing_dir, dep_opt) in self.missing_deps.iter_mut() {
                    ui.strong(missing_dir);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("Required By: {}", dep_opt.required_by));
                    });
                    ui.add_space(5.0);
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut dep_opt.ignore, "Ignore");
                        ui.add_enabled_ui(!dep_opt.ignore, |ui| {
                            // select from installed addons
                            ui.label("Already Installed:");
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
                                ui.label("Install Suggested:");
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
        response
    }
}
