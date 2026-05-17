use std::collections::HashMap;

use super::{
    ResetView, View,
    ui_helpers::{AddonResponse, AddonResponseType, PromisedValue, truncate_len, ui_show_star},
};
use bbcode_egui::{BBState, BBView};
use eframe::egui::{self, Image, Layout, RichText, ScrollArea, vec2};
use egui::Button;
use eso_addons_core::service::{
    AddonService,
    result::{AddonDependencyView, AddonImageResult, AddonShowDetails, Resolution},
};

#[derive(PartialEq, Default)]
enum DetailView {
    #[default]
    Description,
    ChangeLog,
    Pictures,
    FileInfo,
    Dependencies,
    Dependents,
}

#[derive(Default)]
struct DepRowState {
    selected_suggestion: Option<i32>,
    selected_satisfied_by: Option<i32>,
}

#[derive(Default)]
pub struct Details {
    addon_id: i32,
    details: PromisedValue<Option<AddonShowDetails>>,
    view: DetailView,
    show_raw_text: bool,
    bb_description: Option<BBView>,
    bb_description_state: BBState,
    bb_changelog: Option<BBView>,
    bb_changelog_state: BBState,
    images: PromisedValue<Vec<AddonImageResult>>,
    selected_image: String,
    dep_view: PromisedValue<AddonDependencyView>,
    dep_mutation: PromisedValue<()>,
    dep_mutation_was_install: bool,
    pending_addons_changed: bool,
    row_state: HashMap<String, DepRowState>,
}

impl Details {
    fn poll(&mut self, service: &mut AddonService) {
        self.details
            .poll_recording(service, "Loading addon details");
        if self.details.is_ready() {
            self.details.handle();
            self.build_bb_views();
        }
        self.images.poll_recording(service, "Loading addon images");
        self.dep_view
            .poll_recording(service, "Loading addon dependencies");
        if self.dep_view.is_ready() {
            self.dep_view.handle();
        }
        self.dep_mutation
            .poll_recording(service, "Updating addon dependencies");
        if self.dep_mutation.is_ready() {
            self.dep_mutation.handle();
            self.dep_view
                .set(service.get_addon_dependency_view(self.addon_id));
            self.row_state.clear();
            if self.dep_mutation_was_install {
                self.dep_mutation_was_install = false;
                self.pending_addons_changed = true;
            }
        }
    }

    fn build_bb_views(&mut self) {
        if self.bb_description.is_some() && self.bb_changelog.is_some() {
            return;
        }
        let Some(Some(addon)) = self.details.value.as_ref() else {
            return;
        };
        let desc = addon.description.as_deref().unwrap_or("");
        let cl = addon.change_log.as_deref().unwrap_or("");
        self.bb_description = Some(BBView::parse(desc));
        self.bb_changelog = Some(BBView::parse(cl));
    }

    pub fn set_addon(&mut self, addon_id: i32, service: &mut AddonService) {
        self.addon_id = addon_id;
        self.details.set(service.get_addon_details(addon_id));
        self.images.set(service.get_addon_images(addon_id));
        self.dep_view
            .set(service.get_addon_dependency_view(addon_id));
        self.view = DetailView::default();
        self.selected_image = String::default();
        self.bb_description = None;
        self.bb_changelog = None;
        self.bb_description_state = BBState::default();
        self.bb_changelog_state = BBState::default();
        self.row_state.clear();
        self.dep_mutation_was_install = false;
        self.pending_addons_changed = false;
    }
}
impl View for Details {
    fn ui(
        &mut self,
        _ctx: &egui::Context,
        ui: &mut egui::Ui,
        service: &mut AddonService,
    ) -> AddonResponse {
        let mut response = AddonResponse::default();
        self.poll(service);

        if self.pending_addons_changed {
            self.pending_addons_changed = false;
            response.response_type = AddonResponseType::AddonsChanged;
            return response;
        }

        if self.details.is_polling() {
            ui.spinner();
            return response;
        }

        let Some(Some(addon)) = self.details.value.as_ref() else {
            ui.label("No addon!");
            return response;
        };
        egui::Panel::top("detail_top").show_inside(ui, |ui| {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                //close button
                if ui.button(RichText::new("⮪ Close").heading()).clicked() {
                    response.response_type = AddonResponseType::Close;
                }

                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if addon.is_upgradable()
                        && ui.button(RichText::new("⮉ Update").heading()).clicked()
                    {
                        response.addon_id = addon.id;
                        response.response_type = AddonResponseType::Update;
                    }
                    if !addon.installed {
                        if ui.button(RichText::new("⮋ Install").heading()).clicked() {
                            response.addon_id = addon.id;
                            response.response_type = AddonResponseType::Install
                        }
                    } else if ui.button(RichText::new("🗙 Remove").heading()).clicked() {
                        response.response_type = AddonResponseType::Remove;
                        response.addon_id = addon.id;
                    }
                });
            });
            ui.add_space(5.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new(addon.name.as_str()).heading().strong());
                if addon
                    .download_total
                    .as_ref()
                    .unwrap()
                    .parse::<i32>()
                    .unwrap()
                    > 5000
                {
                    ui_show_star(ui);
                }
            });
            ui.add_space(5.0);

            ui.horizontal(|ui| {
                if ui
                    .selectable_label(
                        false,
                        RichText::new(format!("by: {}", addon.author_name.as_str())),
                    )
                    .clicked()
                {
                    response.author_name = addon.author_name.clone();
                    response.response_type = AddonResponseType::AuthorName;
                }
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.hyperlink_to("Visit Website", &addon.file_info_url);
                });
            });

            ui.horizontal(|ui| {
                // TODO: Add icon
                ui.label(addon.category.as_str());
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    // TODO: pretty format date like: January 1, 2024
                    ui.label(format!("🕘 Updated {}", addon.date));
                });
            });
            ui.horizontal(|ui| {
                if let (Some(name), Some(ver)) =
                    (&addon.game_compat_name, &addon.game_compat_version)
                {
                    ui.label(format!("⛭ {name} ({ver}) Supported"));
                } else {
                    ui.label("⛭ Unknown Version Supported");
                }
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    // TODO: pretty print download count
                    ui.label(format!(
                        "⮋ {} Downloads",
                        addon.download_total.as_deref().unwrap_or("")
                    ));
                });
            });
            ui.horizontal(|ui| {
                let mut version_text = addon.version.clone();
                if let Some(ref installed_version) = addon.installed_version
                    && *installed_version != addon.version
                {
                    version_text = format!("{} ➡ {}", installed_version, addon.version);
                }
                ui.label(format!("🔁 Version {}", version_text));
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    // TODO: pretty print favorite count
                    ui.label(format!(
                        "♥ {} Favorites",
                        addon.favorite_total.as_deref().unwrap_or("")
                    ));
                });
            });
            ui.separator();

            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.view,
                    DetailView::Description,
                    RichText::new("Details").heading(),
                );
                if self.images.is_ready() && !self.images.value.as_ref().unwrap().is_empty() {
                    ui.selectable_value(
                        &mut self.view,
                        DetailView::Pictures,
                        RichText::new("Images").heading(),
                    );
                }
                ui.selectable_value(
                    &mut self.view,
                    DetailView::FileInfo,
                    RichText::new("File Info").heading(),
                );
                ui.selectable_value(
                    &mut self.view,
                    DetailView::ChangeLog,
                    RichText::new("Change Log").heading(),
                );
                let (deps_label, deps_enabled, dependents_label, dependents_enabled) =
                    match self.dep_view.value.as_ref() {
                        Some(v) => (
                            if v.forward.is_empty() {
                                "No Dependencies"
                            } else {
                                "Dependencies"
                            },
                            !v.forward.is_empty(),
                            if v.dependents.is_empty() {
                                "No Dependents"
                            } else {
                                "Dependents"
                            },
                            !v.dependents.is_empty(),
                        ),
                        None => ("Dependencies", true, "Dependents", true),
                    };
                ui.add_enabled_ui(deps_enabled, |ui| {
                    ui.selectable_value(
                        &mut self.view,
                        DetailView::Dependencies,
                        RichText::new(deps_label).heading(),
                    );
                });
                ui.add_enabled_ui(dependents_enabled, |ui| {
                    ui.selectable_value(
                        &mut self.view,
                        DetailView::Dependents,
                        RichText::new(dependents_label).heading(),
                    );
                });
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut self.show_raw_text, "Raw");
                });
            });
            ui.add_space(5.0);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| match self.view {
                    DetailView::Description => {
                        if self.show_raw_text {
                            ui.label(addon.description.as_deref().unwrap_or(""));
                        } else if let Some(view) = &self.bb_description {
                            view.show(ui, &mut self.bb_description_state, "description");
                        }
                    }
                    DetailView::ChangeLog => {
                        if self.show_raw_text {
                            ui.label(addon.change_log.as_deref().unwrap_or(""));
                        } else if let Some(view) = &self.bb_changelog {
                            view.show(ui, &mut self.bb_changelog_state, "change_log");
                        }
                    }
                    DetailView::Pictures => {
                        if self.selected_image == String::default() {
                            // set selected to first image
                            if let Some(img) = self.images.value.as_ref().unwrap().first() {
                                img.image.clone_into(&mut self.selected_image);
                            }
                        }
                        egui::Panel::left("image_left")
                            .default_size(100.0)
                            .show_inside(ui, |ui| {
                                ScrollArea::vertical().show(ui, |ui| {
                                    for image in self.images.value.as_ref().unwrap() {
                                        if ui
                                            .add(Button::image(
                                                Image::new(image.thumbnail.to_owned())
                                                    .fit_to_exact_size(vec2(100.0, 100.0)),
                                            ))
                                            .clicked()
                                        {
                                            image.image.clone_into(&mut self.selected_image);
                                        }
                                    }
                                });
                            });
                        egui::CentralPanel::default().show_inside(ui, |ui| {
                            ui.centered_and_justified(|ui| {
                                if self.selected_image != String::default() {
                                    ui.add(
                                        Image::new(self.selected_image.to_owned()).shrink_to_fit(),
                                    );
                                }
                            });
                        });
                    }
                    DetailView::FileInfo => {
                        egui::Grid::new("my_grid")
                            .num_columns(2)
                            .spacing([40.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                if let Some(download) = &addon.download {
                                    ui.label("Download Link");
                                    ui.hyperlink_to(truncate_len(download, 40), download);
                                    ui.end_row();
                                }
                                if let Some(md5) = &addon.md5 {
                                    ui.label("MD5");
                                    ui.code(md5.as_str());
                                    ui.end_row();
                                }
                            });
                    }
                    DetailView::Dependencies => {
                        let Some(dep_view) = self.dep_view.value.as_ref() else {
                            ui.spinner();
                            return;
                        };
                        let mut action: Option<DepAction> = None;
                        let install_all_items: Vec<(String, i32)> = dep_view
                            .forward
                            .iter()
                            .filter_map(|d| match &d.resolution {
                                Resolution::Unresolved { suggestions } => {
                                    suggestions.first().map(|s| (d.dep_dir.clone(), s.id))
                                }
                                _ => None,
                            })
                            .collect();
                        if !install_all_items.is_empty() {
                            ui.horizontal(|ui| {
                                if ui
                                    .button(
                                        RichText::new(format!(
                                            "⮋ Install All ({})",
                                            install_all_items.len()
                                        ))
                                        .heading(),
                                    )
                                    .clicked()
                                {
                                    action =
                                        Some(DepAction::InstallBatch(install_all_items.clone()));
                                }
                            });
                            ui.separator();
                        }
                        for dep in &dep_view.forward {
                            ui.horizontal_wrapped(|ui| {
                                ui.strong(format!("{}:", dep.dep_dir));
                                match &dep.resolution {
                                    Resolution::Installed(r) => {
                                        if ui
                                            .selectable_label(
                                                false,
                                                format!("{} (installed)", r.name),
                                            )
                                            .clicked()
                                        {
                                            action = Some(DepAction::Navigate(r.id));
                                        }
                                    }
                                    Resolution::SatisfiedBy(r) => {
                                        ui.label("satisfied by");
                                        if ui.selectable_label(false, &r.name).clicked() {
                                            action = Some(DepAction::Navigate(r.id));
                                        }
                                        if ui.button("revoke").clicked() {
                                            action = Some(DepAction::Revoke(dep.dep_dir.clone()));
                                        }
                                    }
                                    Resolution::Ignored => {
                                        ui.label("ignored");
                                        if ui.button("revoke").clicked() {
                                            action = Some(DepAction::Revoke(dep.dep_dir.clone()));
                                        }
                                    }
                                    Resolution::Unresolved { suggestions } => {
                                        let row =
                                            self.row_state.entry(dep.dep_dir.clone()).or_default();
                                        if row.selected_suggestion.is_none() {
                                            row.selected_suggestion =
                                                suggestions.first().map(|s| s.id);
                                        }
                                        if ui.button("Ignore").clicked() {
                                            action =
                                                Some(DepAction::SetIgnored(dep.dep_dir.clone()));
                                        }
                                        ui.label("satisfied by:");
                                        let sb_text = row
                                            .selected_satisfied_by
                                            .and_then(|id| {
                                                dep_view
                                                    .installed_addons
                                                    .iter()
                                                    .find(|a| a.id == id)
                                                    .map(|a| a.name.as_str())
                                            })
                                            .unwrap_or("");
                                        egui::ComboBox::from_id_salt(format!("sb_{}", dep.dep_dir))
                                            .selected_text(sb_text)
                                            .width(180.0)
                                            .show_ui(ui, |ui| {
                                                for a in &dep_view.installed_addons {
                                                    if ui
                                                        .selectable_label(
                                                            row.selected_satisfied_by == Some(a.id),
                                                            &a.name,
                                                        )
                                                        .clicked()
                                                    {
                                                        row.selected_satisfied_by = Some(a.id);
                                                        action = Some(DepAction::SetSatisfiedBy(
                                                            dep.dep_dir.clone(),
                                                            a.id,
                                                        ));
                                                    }
                                                }
                                            });
                                        if !suggestions.is_empty() {
                                            ui.label("install:");
                                            let suggestion_text = row
                                                .selected_suggestion
                                                .and_then(|id| {
                                                    suggestions
                                                        .iter()
                                                        .find(|s| s.id == id)
                                                        .map(|s| s.name.as_str())
                                                })
                                                .unwrap_or("");
                                            egui::ComboBox::from_id_salt(format!(
                                                "sg_{}",
                                                dep.dep_dir
                                            ))
                                            .selected_text(suggestion_text)
                                            .width(180.0)
                                            .show_ui(
                                                ui,
                                                |ui| {
                                                    for s in suggestions {
                                                        if ui
                                                            .selectable_label(
                                                                row.selected_suggestion
                                                                    == Some(s.id),
                                                                &s.name,
                                                            )
                                                            .clicked()
                                                        {
                                                            row.selected_suggestion = Some(s.id);
                                                        }
                                                    }
                                                },
                                            );
                                            if ui.button("Install").clicked()
                                                && let Some(id) = row.selected_suggestion
                                            {
                                                action = Some(DepAction::InstallBatch(vec![(
                                                    dep.dep_dir.clone(),
                                                    id,
                                                )]));
                                            }
                                        }
                                    }
                                }
                            });
                            ui.separator();
                        }
                        if let Some(action) = action {
                            match action {
                                DepAction::Navigate(id) => {
                                    response.addon_id = id;
                                    response.response_type = AddonResponseType::AddonName;
                                }
                                DepAction::SetIgnored(dir) => {
                                    self.dep_mutation.set(service.set_dep_ignored(dir));
                                }
                                DepAction::SetSatisfiedBy(dir, id) => {
                                    self.dep_mutation.set(service.set_dep_satisfied_by(dir, id));
                                }
                                DepAction::Revoke(dir) => {
                                    self.dep_mutation.set(service.revoke_dep_override(dir));
                                }
                                DepAction::InstallBatch(items) => {
                                    self.dep_mutation
                                        .set(service.install_dep_suggestions(items));
                                    self.dep_mutation_was_install = true;
                                }
                            }
                        }
                    }
                    DetailView::Dependents => {
                        let Some(dep_view) = self.dep_view.value.as_ref() else {
                            ui.spinner();
                            return;
                        };
                        let count = dep_view.dependents.len();
                        ui.label(format!(
                            "Required by {} installed addon{}:",
                            count,
                            if count == 1 { "" } else { "s" }
                        ));
                        ui.add_space(5.0);
                        let mut nav: Option<i32> = None;
                        for r in &dep_view.dependents {
                            if ui.selectable_label(false, &r.name).clicked() {
                                nav = Some(r.id);
                            }
                        }
                        if let Some(id) = nav {
                            response.addon_id = id;
                            response.response_type = AddonResponseType::AddonName;
                        }
                    }
                });
        });
        response
    }
}

enum DepAction {
    Navigate(i32),
    SetIgnored(String),
    SetSatisfiedBy(String, i32),
    Revoke(String),
    InstallBatch(Vec<(String, i32)>),
}
impl ResetView for Details {
    fn reset(&mut self, service: &mut AddonService) {
        // do not get if not addon id set yet
        if self.addon_id == i32::default() {
            return;
        }
        // re-get details for same addon
        self.details.set(service.get_addon_details(self.addon_id));
        self.dep_view
            .set(service.get_addon_dependency_view(self.addon_id));
        self.row_state.clear();
        self.bb_description = None;
        self.bb_changelog = None;
    }
}
