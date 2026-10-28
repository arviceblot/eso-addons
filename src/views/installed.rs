use tracing::log::info;

use eframe::egui::{self, Layout, RichText, TextWrapMode};
use eso_addons_core::service::AddonService;
use eso_addons_core::service::result::AddonShowDetails;
use strum::IntoEnumIterator;

use super::{
    View,
    ui_helpers::{
        AddonResponse, AddonResponseType, InstalledAddonRow, InstalledAddonSource,
        InstalledAddonTable, InstalledSort,
    },
};

const LIBRARY_CATEGORY: &str = "Libraries";

/// Filter by source in the unified installed view.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum SourceFilter {
    #[default]
    All,
    Esoui,
    Local,
    Git,
}

impl SourceFilter {
    fn label(&self) -> &'static str {
        match self {
            SourceFilter::All => "All",
            SourceFilter::Esoui => "ESOUI",
            SourceFilter::Local => "Local",
            SourceFilter::Git => "Git",
        }
    }

    fn iter() -> impl Iterator<Item = SourceFilter> {
        [
            SourceFilter::All,
            SourceFilter::Esoui,
            SourceFilter::Local,
            SourceFilter::Git,
        ]
        .into_iter()
    }

    fn matches(&self, source: &InstalledAddonSource) -> bool {
        matches!(
            (self, source),
            (SourceFilter::All, _)
                | (SourceFilter::Esoui, InstalledAddonSource::Esoui { .. })
                | (SourceFilter::Local, InstalledAddonSource::Local { .. })
                | (SourceFilter::Git, InstalledAddonSource::Git { .. })
        )
    }
}

impl std::fmt::Display for SourceFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[derive(Default)]
pub struct Installed {
    // -- Unified addon list --
    addons: Vec<InstalledAddonRow>,
    filter: String,
    sort: InstalledSort,
    prev_sort: InstalledSort,
    ascending: bool,
    prev_ascending: bool,

    // -- Source filter --
    source_filter: SourceFilter,
}

impl Installed {
    pub fn new() -> Installed {
        Installed {
            addons: vec![],
            filter: Default::default(),
            sort: InstalledSort::Name,
            prev_sort: InstalledSort::Id,
            ascending: InstalledSort::Name.default_ascending(),
            prev_ascending: InstalledSort::Name.default_ascending(),
            source_filter: SourceFilter::All,
        }
    }

    /// Set addons from all three sources.
    pub fn set_addons(
        mut self,
        esoui: Vec<AddonShowDetails>,
        local: Vec<InstalledAddonRow>,
        git: Vec<InstalledAddonRow>,
    ) -> Self {
        let mut all: Vec<InstalledAddonRow> = esoui.into_iter().map(Into::into).collect();
        all.extend(local);
        all.extend(git);

        self.addons = all;
        self.sort_addons();
        self
    }

    // -- Sort/filter helpers --

    fn sort_addons(&mut self) {
        info!("Sorting installed addons");
        super::ui_helpers::sort_installed_addons(&mut self.addons, self.sort, self.ascending);
    }

    fn get_updateable_count(&self) -> usize {
        self.addons.iter().filter(|x| x.is_upgradable).count()
    }

    /// Get ESOUI addon IDs that are upgradable.
    fn get_upgradable_esoui_ids(&self) -> Vec<i64> {
        self.addons
            .iter()
            .filter(|a| a.is_upgradable && matches!(a.source, InstalledAddonSource::Esoui { .. }))
            .map(|a| a.id)
            .collect()
    }

    /// Get Git repo IDs that are upgradable.
    fn get_upgradable_git_repo_ids(&self) -> Vec<i64> {
        self.addons
            .iter()
            .filter_map(|a| {
                if !a.is_upgradable {
                    return None;
                }
                match &a.source {
                    InstalledAddonSource::Git { git_addon_id, .. } => Some(*git_addon_id),
                    _ => None,
                }
            })
            .collect()
    }
}

impl View for Installed {
    fn ui(
        &mut self,
        _ctx: &egui::Context,
        ui: &mut egui::Ui,
        service: &mut AddonService,
    ) -> AddonResponse {
        let mut response = AddonResponse::default();

        let total = self.addons.len();
        if total == 0 {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.heading("No addons installed!");
                })
            });
            return response;
        }

        // Top panel: title and actions
        // Compute folder breadcrumb outside egui render closure to avoid
        // block_in_place interfering with layout.
        let folder_breadcrumb = {
            let active_dir = service.get_active_addon_dir_sync();
            active_dir
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| format!("📁 {n}"))
        };
        egui::Panel::top("installed_top").show(ui, |ui| {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Installed - {total} addons")).heading());
                if let Some(breadcrumb) = &folder_breadcrumb {
                    ui.label(RichText::new(breadcrumb).small().weak());
                }
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    let updateable_count = self.get_updateable_count();
                    if updateable_count > 0
                        && ui.button(RichText::new("⮉ Update All").heading()).clicked()
                    {
                        let update_ids = self.get_upgradable_esoui_ids();
                        response.response_type = AddonResponseType::UpdateMultiple;
                        response.addon_ids = update_ids;
                        let _git_repo_ids = self.get_upgradable_git_repo_ids();
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
        });

        // shortcut any overriding response before content (update, etc.)
        if response.response_type != AddonResponseType::default() {
            return response;
        }

        // Content panel
        egui::CentralPanel::default().show(ui, |ui| {
            response = render_installed_content(
                ui,
                &mut self.addons,
                &mut self.filter,
                &mut self.sort,
                &mut self.prev_sort,
                &mut self.ascending,
                &mut self.prev_ascending,
                &mut self.source_filter,
            );
        });

        response
    }
}

/// Render the unified installed addons content.
#[allow(clippy::too_many_arguments)]
fn render_installed_content(
    ui: &mut egui::Ui,
    addons: &mut [InstalledAddonRow],
    filter: &mut String,
    sort: &mut InstalledSort,
    prev_sort: &mut InstalledSort,
    ascending: &mut bool,
    prev_ascending: &mut bool,
    source_filter: &mut SourceFilter,
) -> AddonResponse {
    let mut response = AddonResponse::default();

    if addons.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new("No addons installed.").weak());
        });
        return response;
    }

    // Handle sort changes (may have been triggered by header clicks last frame)
    let sort_changed = *prev_sort != *sort || *prev_ascending != *ascending;
    if *prev_sort != *sort {
        *ascending = sort.default_ascending();
    }
    if sort_changed {
        super::ui_helpers::sort_installed_addons(addons, *sort, *ascending);
    }
    *prev_sort = *sort;
    *prev_ascending = *ascending;

    // Sort, source filter, and text filter
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("installed_sort")
            .selected_text(format!("Sort: {}", sort.to_string().to_uppercase()))
            .show_ui(ui, |ui| {
                ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                ui.set_min_width(60.0);
                for s in InstalledSort::iter() {
                    ui.selectable_value(sort, s, s.to_string());
                }
            });
        egui::ComboBox::from_id_salt("installed_source")
            .selected_text(format!(
                "Source: {}",
                source_filter.to_string().to_uppercase()
            ))
            .show_ui(ui, |ui| {
                ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                ui.set_min_width(60.0);
                for sf in SourceFilter::iter() {
                    let count = addons.iter().filter(|a| sf.matches(&a.source)).count();
                    let label = format!("{} ({})", sf.label(), count);
                    ui.selectable_value(source_filter, sf, label);
                }
            });
        ui.add(egui::TextEdit::singleline(filter).hint_text("Search installed addons ..."));
        if ui.button("🗙").clicked() {
            filter.clear();
        }
    });
    ui.add_space(3.0);

    // Apply source filter and text filter
    let filter_lower = filter.to_lowercase();
    let matched: Vec<&InstalledAddonRow> = addons
        .iter()
        .filter(|a| {
            source_filter.matches(&a.source)
                && a.name.to_lowercase().contains(filter_lower.as_str())
        })
        .collect();

    if matched.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new("No matching addons.").weak());
        });
        return response;
    }

    // Split libraries and regular addons for display
    if filter_lower.is_empty() {
        let (libraries, regular): (Vec<&InstalledAddonRow>, Vec<&InstalledAddonRow>) = matched
            .into_iter()
            .partition(|a| a.category == LIBRARY_CATEGORY);

        // Render libraries in a separate bottom panel to avoid widget ID conflicts
        if !libraries.is_empty() {
            let lib_addons: Vec<InstalledAddonRow> =
                libraries.iter().map(|a| (*a).clone()).collect();
            egui::Panel::bottom("installed_libraries")
                .resizable(true)
                .default_size(200.0)
                .show(ui, |ui| {
                    ui.add_space(5.0);
                    ui.heading(format!("Libraries - {} addons", libraries.len()));
                    ui.add_space(5.0);
                    let lib_response =
                        InstalledAddonTable::new(&lib_addons).ui(ui, sort, ascending);
                    if lib_response.response_type != AddonResponseType::default() {
                        response = lib_response;
                    }
                });
        }

        let reg_addons: Vec<InstalledAddonRow> = regular.iter().map(|a| (*a).clone()).collect();
        let table_response = InstalledAddonTable::new(&reg_addons).ui(ui, sort, ascending);
        if table_response.response_type != AddonResponseType::default() {
            response = table_response;
        }
    } else {
        let cloned: Vec<InstalledAddonRow> = matched.iter().map(|a| (*a).clone()).collect();
        let table_response = InstalledAddonTable::new(&cloned).ui(ui, sort, ascending);
        if table_response.response_type != AddonResponseType::default() {
            response = table_response;
        }
    }

    response
}
