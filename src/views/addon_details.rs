use std::collections::{HashMap, HashSet};

use super::{
    ResetView, View,
    ui_helpers::{
        AddonResponse, AddonResponseType, InstalledAddonSource, PromisedValue, truncate_len,
        ui_show_star,
    },
};
use bbcode_egui::{BBState, BBView};
use eframe::egui::{self, Image, Layout, RichText, ScrollArea, vec2};
use egui::Button;
use eso_addons_core::service::{
    AddonService,
    result::{
        AddonDependencyView, AddonImageResult, AddonShowDetails, ManifestAddonDetails, Resolution,
    },
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
    selected_suggestion: Option<i64>,
    selected_satisfied_by: Option<i64>,
}

#[derive(PartialEq, Default)]
enum DetailMode {
    #[default]
    Esoui,
    Manifest,
}

#[derive(Default)]
pub struct Details {
    /// Which mode the detail view is currently in
    mode: DetailMode,
    addon_id: i64,
    details: PromisedValue<Option<AddonShowDetails>>,
    /// Manifest data for local/git addons
    manifest_addon: Option<ManifestAddonDetails>,
    /// Installed addon directories for resolving manifest dependencies
    manifest_dep_dirs: PromisedValue<HashSet<String>>,
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
    reset_scroll: bool,
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

    pub fn set_addon(&mut self, addon_id: i64, service: &mut AddonService) {
        self.mode = DetailMode::Esoui;
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
        self.reset_scroll = true;
    }

    pub fn set_manifest_addon(&mut self, addon: ManifestAddonDetails, service: &mut AddonService) {
        self.mode = DetailMode::Manifest;
        self.manifest_addon = Some(addon);
        self.manifest_dep_dirs
            .set(service.get_installed_addon_directories());
        self.view = DetailView::default();
        self.reset_scroll = true;
    }

    fn render_manifest(
        &mut self,
        ui: &mut egui::Ui,
        service: &mut AddonService,
        response: &mut AddonResponse,
    ) {
        let Some(addon) = self.manifest_addon.as_ref() else {
            ui.label("No addon!");
            return;
        };

        // Poll the installed directories for dependency resolution
        self.manifest_dep_dirs
            .poll_recording(service, "Loading installed directories");

        // Build the full addon directory path
        let addon_dir = service.get_addon_dir();
        let full_path = addon_dir.join(&addon.directory);
        let full_path_str = full_path.to_string_lossy().to_string();

        let has_deps = !addon.depends_on.is_empty() || !addon.optional_depends_on.is_empty();

        // Default to Dependencies tab if no description and deps exist
        let has_description = addon.description.as_ref().is_some_and(|d| !d.is_empty());
        if !has_description && has_deps {
            self.view = DetailView::Dependencies;
        }

        let mut scroll_area = ScrollArea::vertical().auto_shrink([false, true]);
        let mut tab_changed = false;

        egui::Panel::top("detail_top").show(ui, |ui| {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new("⮪ Close").heading()).clicked() {
                    response.response_type = AddonResponseType::Close;
                }

                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    // Promote button for local addons with ESOUI match
                    if addon.repo_id.is_none()
                        && addon.esoui_match_id.is_some()
                        && ui
                            .button(RichText::new("🔗 Link to ESOUI").heading())
                            .clicked()
                    {
                        response.local_addon_id = addon.id;
                        response.addon_id = addon.esoui_match_id.unwrap_or(0);
                        response.response_type = AddonResponseType::PromoteLocalToEsoui;
                    }
                    if addon.is_upgradable && ui.button(RichText::new("⮉ Pull").heading()).clicked()
                    {
                        response.git_repo_id = addon.repo_id.unwrap_or(0);
                        response.response_type = AddonResponseType::PullGitRepo;
                    }
                    if ui.button(RichText::new("🗙 Remove").heading()).clicked() {
                        if addon.repo_id.is_some() {
                            response.git_addon_id = addon.id;
                            response.response_type = AddonResponseType::RemoveGitAddon;
                        } else {
                            response.local_addon_id = addon.id;
                            response.response_type = AddonResponseType::RemoveLocal;
                        }
                    }
                });
            });
            ui.add_space(5.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new(&addon.title).heading().strong());
            });
            ui.add_space(5.0);

            if let Some(ref author) = addon.author {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("by: {}", author)));
                });
            }

            ui.horizontal(|ui| {
                ui.label(&addon.category);
            });

            if let Some(ref version) = addon.version {
                ui.horizontal(|ui| {
                    ui.label(format!("🔁 Version {}", version));
                });
            }

            if let Some(add_on_version) = addon.add_on_version {
                ui.horizontal(|ui| {
                    ui.label(format!("⛭ Add-on Version: {}", add_on_version));
                });
            }

            if let Some(api_version) = addon.api_version {
                ui.horizontal(|ui| {
                    ui.label(format!("🔌 API Version: {}", api_version));
                });
            }

            // Tab bar
            ui.separator();
            ui.horizontal(|ui| {
                tab_changed |= ui
                    .selectable_value(
                        &mut self.view,
                        DetailView::Description,
                        RichText::new("Details").heading(),
                    )
                    .changed();
                if has_deps
                    && ui
                        .selectable_value(
                            &mut self.view,
                            DetailView::Dependencies,
                            RichText::new("Dependencies").heading(),
                        )
                        .clicked()
                {
                    tab_changed = true;
                } else if !has_deps {
                    ui.add_enabled(false, {
                        let label = RichText::new("Dependencies").heading();
                        egui::Label::new(label).wrap()
                    });
                }
            });
            ui.add_space(5.0);
        });

        if self.reset_scroll || tab_changed {
            scroll_area = scroll_area.vertical_scroll_offset(0.0);
            self.reset_scroll = false;
        }

        egui::CentralPanel::default().show(ui, |ui| {
            scroll_area.show(ui, |ui| match self.view {
                DetailView::Description => {
                    // Description
                    if let Some(ref desc) = addon.description
                        && !desc.is_empty()
                    {
                        ui.label(desc);
                        ui.add_space(10.0);
                    }

                    // Addon directory path
                    ui.label(RichText::new("Directory").heading());
                    ui.separator();
                    egui::Grid::new("manifest_dir_grid")
                        .num_columns(2)
                        .spacing([40.0, 4.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("Path");
                            ui.code(&full_path_str);
                            ui.end_row();
                        });
                    ui.add_space(10.0);

                    // Git URL (only for git addons)
                    if let Some(ref repo_url) = addon.repo_url {
                        ui.label(RichText::new("Repository").heading());
                        ui.separator();
                        egui::Grid::new("manifest_repo_grid")
                            .num_columns(2)
                            .spacing([40.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                ui.label("URL");
                                ui.hyperlink_to(truncate_len(repo_url, 60), repo_url);
                                ui.end_row();
                            });
                    }
                }
                DetailView::Dependencies => {
                    if self.manifest_dep_dirs.is_polling() {
                        ui.spinner();
                    } else if let Some(ref dirs) = self.manifest_dep_dirs.value {
                        // Helper: check if a dependency directory name is installed
                        let is_installed = |dep_name: &str| -> bool {
                            dirs.iter().any(|d| d.eq_ignore_ascii_case(dep_name))
                        };

                        // Required dependencies
                        for dep in &addon.depends_on {
                            ui.horizontal_wrapped(|ui| {
                                ui.strong(format!("{}:", dep));
                                if is_installed(dep) {
                                    ui.label(format!("{} (installed)", dep));
                                } else {
                                    ui.label(RichText::new("missing").color(egui::Color32::RED));
                                }
                            });
                            ui.separator();
                        }

                        // Optional dependencies
                        for dep in &addon.optional_depends_on {
                            ui.horizontal_wrapped(|ui| {
                                ui.strong(format!("{} (optional):", dep));
                                if is_installed(dep) {
                                    ui.label(format!("{} (installed)", dep));
                                } else {
                                    ui.label("not installed");
                                }
                            });
                            ui.separator();
                        }

                        if !has_deps {
                            ui.label("No dependencies.");
                        }
                    } else {
                        // Fallback: just list names without resolution
                        for dep in &addon.depends_on {
                            ui.label(format!("• {}", dep));
                        }
                        for dep in &addon.optional_depends_on {
                            ui.label(format!("• {} (optional)", dep));
                        }

                        if !has_deps {
                            ui.label("No dependencies.");
                        }
                    }
                }
                _ => {}
            });
        });
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

        // Manifest mode (local/git addons)
        if matches!(self.mode, DetailMode::Manifest) {
            self.render_manifest(ui, service, &mut response);
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

        let mut scroll_area = ScrollArea::vertical().auto_shrink([false, true]);
        let mut tab_changed = false;

        egui::Panel::top("detail_top").show(ui, |ui| {
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
                    .parse::<i64>()
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
                tab_changed |= ui
                    .selectable_value(
                        &mut self.view,
                        DetailView::Description,
                        RichText::new("Details").heading(),
                    )
                    .changed();
                if self.images.is_ready() && !self.images.value.as_ref().unwrap().is_empty() {
                    tab_changed |= ui
                        .selectable_value(
                            &mut self.view,
                            DetailView::Pictures,
                            RichText::new("Images").heading(),
                        )
                        .changed();
                }
                tab_changed |= ui
                    .selectable_value(
                        &mut self.view,
                        DetailView::FileInfo,
                        RichText::new("File Info").heading(),
                    )
                    .changed();
                tab_changed |= ui
                    .selectable_value(
                        &mut self.view,
                        DetailView::ChangeLog,
                        RichText::new("Change Log").heading(),
                    )
                    .changed();
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
                    tab_changed |= ui
                        .selectable_value(
                            &mut self.view,
                            DetailView::Dependencies,
                            RichText::new(deps_label).heading(),
                        )
                        .changed();
                });
                ui.add_enabled_ui(dependents_enabled, |ui| {
                    tab_changed |= ui
                        .selectable_value(
                            &mut self.view,
                            DetailView::Dependents,
                            RichText::new(dependents_label).heading(),
                        )
                        .changed();
                });
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut self.show_raw_text, "Raw");
                });
            });
            ui.add_space(5.0);
        });

        if self.reset_scroll || tab_changed {
            scroll_area = scroll_area.vertical_scroll_offset(0.0);
            self.reset_scroll = false;
        }

        egui::CentralPanel::default().show(ui, |ui| {
            scroll_area.show(ui, |ui| match self.view {
                DetailView::Description => {
                    if self.show_raw_text {
                        ui.label(addon.description.as_deref().unwrap_or(""));
                    } else if let Some(view) = &self.bb_description {
                        view.show(ui, &mut self.bb_description_state, "description");
                        if let Some(url) = self.bb_description_state.take_clicked_link() {
                            handle_bb_link(ui, &url, &mut response);
                        }
                    }
                }
                DetailView::ChangeLog => {
                    if self.show_raw_text {
                        ui.label(addon.change_log.as_deref().unwrap_or(""));
                    } else if let Some(view) = &self.bb_changelog {
                        view.show(ui, &mut self.bb_changelog_state, "change_log");
                        if let Some(url) = self.bb_changelog_state.take_clicked_link() {
                            handle_bb_link(ui, &url, &mut response);
                        }
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
                        .show(ui, |ui| {
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
                    egui::CentralPanel::default().show(ui, |ui| {
                        ui.centered_and_justified(|ui| {
                            if self.selected_image != String::default() {
                                ui.add(Image::new(self.selected_image.to_owned()).shrink_to_fit());
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
                    let install_all_items: Vec<(String, i64)> = dep_view
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
                                action = Some(DepAction::InstallBatch(install_all_items.clone()));
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
                                        .selectable_label(false, format!("{} (installed)", r.name))
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
                                        row.selected_suggestion = suggestions.first().map(|s| s.id);
                                    }
                                    if ui.button("Ignore").clicked() {
                                        action = Some(DepAction::SetIgnored(dep.dep_dir.clone()));
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
                                        egui::ComboBox::from_id_salt(format!("sg_{}", dep.dep_dir))
                                            .selected_text(suggestion_text)
                                            .width(180.0)
                                            .show_ui(ui, |ui| {
                                                for s in suggestions {
                                                    if ui
                                                        .selectable_label(
                                                            row.selected_suggestion == Some(s.id),
                                                            &s.name,
                                                        )
                                                        .clicked()
                                                    {
                                                        row.selected_suggestion = Some(s.id);
                                                    }
                                                }
                                            });
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
                                response.source = InstalledAddonSource::Esoui { esoui_id: id };
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
                    let mut nav: Option<i64> = None;
                    for r in &dep_view.dependents {
                        if ui.selectable_label(false, &r.name).clicked() {
                            nav = Some(r.id);
                        }
                    }
                    if let Some(id) = nav {
                        response.addon_id = id;
                        response.source = InstalledAddonSource::Esoui { esoui_id: id };
                        response.response_type = AddonResponseType::AddonName;
                    }
                }
            });
        });
        response
    }
}

/// Navigate to the in-app detail view when a bbcode link points at another
/// addon on esoui, otherwise open it in the browser.
fn handle_bb_link(ui: &egui::Ui, url: &str, response: &mut AddonResponse) {
    match esoui_addon_id(url) {
        Some(id) => {
            response.addon_id = id;
            response.source = InstalledAddonSource::Esoui { esoui_id: id };
            response.response_type = AddonResponseType::AddonName;
        }
        None => ui.ctx().open_url(egui::OpenUrl::new_tab(url.to_string())),
    }
}

/// Extract the addon id from an esoui file info URL, matching the forms
/// `esoui.com/downloads/info<id>-<slug>.html` and
/// `esoui.com/downloads/fileinfo.php?id=<id>`.
fn esoui_addon_id(url: &str) -> Option<i64> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let (host, path) = rest.split_once('/')?;
    let host = host.strip_prefix("www.").unwrap_or(host);
    if !host.eq_ignore_ascii_case("esoui.com") {
        return None;
    }
    let last = path.rsplit('/').next().unwrap_or(path);
    if let Some(num) = last.strip_prefix("info") {
        return leading_i64(num);
    }
    path.split('?')
        .nth(1)?
        .split('&')
        .find_map(|kv| kv.strip_prefix("id="))
        .and_then(leading_i64)
}

fn leading_i64(s: &str) -> Option<i64> {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    s.get(..end).filter(|d| !d.is_empty())?.parse().ok()
}

enum DepAction {
    Navigate(i64),
    SetIgnored(String),
    SetSatisfiedBy(String, i64),
    Revoke(String),
    InstallBatch(Vec<(String, i64)>),
}
impl ResetView for Details {
    fn reset(&mut self, _service: &mut AddonService) {
        match self.mode {
            DetailMode::Esoui => {
                if self.addon_id == i64::default() {
                    return;
                }
                // re-get details for same addon
                self.details.set(_service.get_addon_details(self.addon_id));
                self.dep_view
                    .set(_service.get_addon_dependency_view(self.addon_id));
                self.row_state.clear();
                self.bb_description = None;
                self.bb_changelog = None;
            }
            DetailMode::Manifest => {
                // Manifest data is refreshed on next scan; nothing to do here.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::esoui_addon_id;

    #[test]
    fn parses_info_url() {
        assert_eq!(
            esoui_addon_id("https://www.esoui.com/downloads/info1360-LazyWritCrafter.html"),
            Some(1360)
        );
    }

    #[test]
    fn parses_fileinfo_query() {
        assert_eq!(
            esoui_addon_id("https://www.esoui.com/downloads/fileinfo.php?id=1360"),
            Some(1360)
        );
        assert_eq!(
            esoui_addon_id("http://esoui.com/downloads/fileinfo.php?foo=1&id=42"),
            Some(42)
        );
    }

    #[test]
    fn rejects_other_hosts_and_pages() {
        assert_eq!(esoui_addon_id("https://github.com/info123-foo.html"), None);
        assert_eq!(esoui_addon_id("https://www.esoui.com/downloads/"), None);
        assert_eq!(
            esoui_addon_id("https://www.esoui.com/downloads/cat1-Combat.html"),
            None
        );
    }
}
