use std::fmt;

use eframe::{
    egui::{
        self, Image, RichText, TextFormat, TextStyle,
        text::{LayoutJob, TextWrapping},
        vec2,
    },
    epaint::Color32,
};
use eso_addons_core::service::AddonService;
use eso_addons_core::service::result::{AddonShowDetails, MissingDepView};
use lazy_async_promise::{DirectCacheAccess, ImmediateValuePromise, ImmediateValueState};
use strum_macros::EnumIter;

#[derive(
    Debug, PartialEq, Clone, Copy, EnumIter, serde::Deserialize, serde::Serialize, Default,
)]
pub enum Sort {
    Name,
    Updated,
    Author,
    TotalDownloads,
    MonthlyDownloads,
    Favorites,
    #[default]
    Id,
}
impl fmt::Display for Sort {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Sort::Name => write!(f, "Name"),
            Sort::Updated => write!(f, "Date"),
            Sort::Author => write!(f, "Author"),
            Sort::TotalDownloads => write!(f, "Total Downloads"),
            Sort::MonthlyDownloads => write!(f, "Monthly Downloads"),
            Sort::Favorites => write!(f, "Likes"),
            Sort::Id => write!(f, "ID"),
        }
    }
}
impl Sort {
    /// Default direction when a column first becomes the active sort: names
    /// ascending, counts and dates with the largest/newest first.
    pub fn default_ascending(&self) -> bool {
        matches!(self, Sort::Name | Sort::Author | Sort::Id)
    }
}

pub fn sort_addons(addons: &mut [AddonShowDetails], sort: Sort, ascending: bool) {
    let count =
        |value: &Option<String>| value.as_deref().unwrap_or("0").parse::<i64>().unwrap_or(0);
    match sort {
        Sort::Author => addons.sort_by_key(|a| a.author_name.to_lowercase()),
        Sort::Name => addons.sort_by_key(|a| a.name.to_lowercase()),
        Sort::Updated => addons.sort_by(|a, b| a.date.cmp(&b.date)),
        Sort::TotalDownloads => addons.sort_by_key(|a| count(&a.download_total)),
        Sort::MonthlyDownloads => addons.sort_by_key(|a| count(&a.download_monthly)),
        Sort::Favorites => addons.sort_by_key(|a| count(&a.favorite_total)),
        Sort::Id => addons.sort_by_key(|a| a.id),
    }
    if !ascending {
        addons.reverse();
    }
    // keep upgradable addons pinned to the top regardless of sort
    addons.sort_by_key(|a| std::cmp::Reverse(a.is_upgradable()));
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Copy, Debug, PartialEq)]
pub enum ViewOpt {
    /// Not really a reachable view, but a base
    Root,
    // Onboard,
    MissingDeps,
    Installed,
    Search,
    Author,
    Settings,
    Details,
    Errors,
    Quit,
}

#[derive(Default)]
pub struct PromisedValue<T: Send + 'static> {
    promise: Option<ImmediateValuePromise<T>>,
    pub value: Option<T>,
    error: Option<String>,
    handled: bool,
}
impl<T: Send + 'static> PromisedValue<T> {
    pub fn poll(&mut self) {
        if self.promise.is_none() {
            return;
        }
        let state = self.promise.as_mut().unwrap().poll_state_mut();
        match state {
            ImmediateValueState::Success(_) => {
                self.value = state.take_value();
                self.promise = None;
            }
            ImmediateValueState::Error(e) => {
                self.error = Some(format!("{}", **e));
                self.promise = None;
            }
            _ => {}
        }
    }
    pub fn poll_recording(&mut self, service: &AddonService, context: &str) {
        self.poll();
        if let Some(err) = self.error.take() {
            service.record_error(context.to_string(), err);
        }
    }
    pub fn set(&mut self, value_promise: ImmediateValuePromise<T>) {
        self.promise = Some(value_promise);
        self.value = None;
        self.error = None;
        self.handled = false;
    }
    pub fn is_polling(&self) -> bool {
        self.promise.is_some() && self.value.is_none()
    }
    pub fn is_ready(&self) -> bool {
        self.promise.is_none() && self.value.is_some() && !self.handled
    }
    pub fn handle(&mut self) {
        self.handled = true;
    }
    /// Reset the handled flag so this promise's result can be processed again.
    /// Use when the result must be deferred (e.g., dependent data not yet available).
    pub fn unhandle(&mut self) {
        self.handled = false;
    }
}

pub fn truncate_len(text: &String, length: usize) -> String {
    if text.len() > length + 4 {
        let mut new_text = text[..length].to_string();
        new_text.push_str(" ...");
        return new_text;
    }
    text.to_string()
}

use egui_extras::{Column, TableBuilder};

#[derive(PartialEq, Default)]
pub enum AddonResponseType {
    #[default]
    None,
    AddonName,
    /// Generic response that the installed addons have changed
    AddonsChanged,
    AuthorName,
    /// Check for updates
    CheckUpdate,
    Update,
    UpdateMultiple,
    Install,
    InstallMissingDeps,
    Remove,
    /// Remove a manually installed (local) addon
    RemoveLocal,
    /// Remove a git-tracked addon
    RemoveGitAddon,
    /// Pull a specific git repo
    PullGitRepo,
    /// Promote a local addon to a managed ESOUI addon
    PromoteLocalToEsoui,
    Close,
}
pub struct AddonResponse {
    pub addon_id: i64,
    pub addon_ids: Vec<i64>,
    pub author_name: String,
    pub response_type: AddonResponseType,
    pub missing_deps: Vec<MissingDepView>,
    /// ID of a local addon (for RemoveLocal)
    pub local_addon_id: i64,
    /// ID of a git addon (for RemoveGitAddon)
    pub git_addon_id: i64,
    /// ID of a git repo (for PullGitRepo)
    pub git_repo_id: i64,
    /// Source of the selected addon (for AddonName detail view)
    pub source: InstalledAddonSource,
}
impl Default for AddonResponse {
    fn default() -> Self {
        Self {
            addon_id: 0,
            addon_ids: vec![],
            response_type: AddonResponseType::default(),
            author_name: "".to_string(),
            missing_deps: vec![],
            local_addon_id: 0,
            git_addon_id: 0,
            git_repo_id: 0,
            source: InstalledAddonSource::Esoui { esoui_id: 0 },
        }
    }
}
pub struct AddonTable<'a> {
    addons: &'a Vec<&'a AddonShowDetails>,
    allow_install: bool,
}
impl<'a> AddonTable<'a> {
    pub fn new(addons: &'a Vec<&'a AddonShowDetails>) -> Self {
        Self {
            addons,
            allow_install: false,
        }
    }
    pub fn installable(mut self, value: bool) -> Self {
        self.allow_install = value;
        self
    }
    pub fn ui(&self, ui: &mut egui::Ui, sort: &mut Sort, ascending: &mut bool) -> AddonResponse {
        let Self {
            addons,
            allow_install,
        } = *self;
        // let has_updateable = any(addons.iter(), |x| x.is_upgradable());
        let num_rows = addons.len();
        let mut response = AddonResponse::default();
        // egui_extras' own scroll area has horizontal scrolling hardcoded off, so
        // wrap the table to provide a horizontal scrollbar once the columns can no
        // longer all fit (see the column definitions below).
        egui::ScrollArea::horizontal().show(ui, |ui| {
            // Size the date and numeric columns to their actual rendered content
            // (header label plus sort arrow, or the widest value) so they're never
            // wider than needed and their titles never truncate.
            let date_width = header_width(ui, "Date").max(body_width(ui, "8888-88-88"));
            let downloads_width = header_width(ui, "Downloads").max(body_width(ui, "88888888"));
            let monthly_width = header_width(ui, "Monthly").max(body_width(ui, "888888"));
            let likes_width = header_width(ui, "Likes").max(body_width(ui, "888888"));

            // Drive the Name column width ourselves rather than using a `remainder`
            // column: egui_extras floors a non-last remainder column at its widest
            // visible content, so a single long name would veto shrinking the window.
            // Sizing from the viewport instead lets Name fill slack yet still collapse
            // to NAME_MIN, truncating long names rather than blocking the resize.
            let spacing_x = ui.spacing().item_spacing.x;
            let fixed = ACTION_COL_WIDTH
                + ICON_WIDTH
                + AUTHOR_WIDTH
                + date_width
                + downloads_width
                + monthly_width
                + likes_width
                + 7.0 * spacing_x;
            let name_width = (ui.available_width() - ui.spacing().scroll.allocated_width() - fixed)
                .max(NAME_MIN);
            TableBuilder::new(ui)
                .auto_shrink(true)
                .striped(true)
                // .resizable(self.resizable)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .sense(egui::Sense::hover())
                .max_scroll_height(3200.0)
                // Fixed widths everywhere keep the layout deterministic so it never
                // reflows as rows scroll into view. Name takes the computed remaining
                // width and clips; once the window shrinks past the point where
                // everything fits, the horizontal ScrollArea kicks in rather than
                // pushing columns off the right edge.
                .column(Column::exact(ACTION_COL_WIDTH))
                .column(Column::exact(ICON_WIDTH))
                .column(Column::exact(name_width).clip(true))
                .column(Column::exact(AUTHOR_WIDTH))
                .column(Column::exact(date_width))
                .column(Column::exact(downloads_width))
                .column(Column::exact(monthly_width))
                .column(Column::exact(likes_width))
                .header(24.0, |mut header| {
                    header.col(|_| {});
                    header.col(|_| {});
                    header.col(|ui| sortable_header(ui, sort, ascending, "Name", Sort::Name));
                    header.col(|ui| sortable_header(ui, sort, ascending, "Author", Sort::Author));
                    header.col(|ui| sortable_header(ui, sort, ascending, "Date", Sort::Updated));
                    header.col(|ui| {
                        right_aligned(ui, |ui| {
                            sortable_header(ui, sort, ascending, "Downloads", Sort::TotalDownloads)
                        })
                    });
                    header.col(|ui| {
                        right_aligned(ui, |ui| {
                            sortable_header(ui, sort, ascending, "Monthly", Sort::MonthlyDownloads)
                        })
                    });
                    header.col(|ui| {
                        right_aligned(ui, |ui| {
                            sortable_header(ui, sort, ascending, "Likes", Sort::Favorites)
                        })
                    });
                })
                .body(|body| {
                    body.rows(ROW_HEIGHT, num_rows, |mut row| {
                        let addon = &addons[row.index()];

                        row.col(|ui| {
                            if !allow_install {
                                return;
                            }
                            // Fixed-size allocation so the cell's layout centers the
                            // button both vertically and horizontally.
                            ui.allocate_ui_with_layout(
                                vec2(ui.available_width(), ACTION_BTN.y),
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().interact_size = vec2(0.0, 0.0);
                                    if !addon.installed {
                                        if action_button(ui, "✚", "Install", COLOR_INSTALL) {
                                            response.addon_id = addon.id;
                                            response.response_type = AddonResponseType::Install;
                                        }
                                        return;
                                    }
                                    if addon.is_upgradable()
                                        && action_button(ui, "⮉", "Update", COLOR_UPDATE)
                                    {
                                        response.addon_id = addon.id;
                                        response.response_type = AddonResponseType::Update;
                                    }
                                },
                            );
                        });

                        row.col(|ui| {
                            if let Some(icon) = &addon.category_icon {
                                ui.add(
                                    Image::new(icon)
                                        .fit_to_exact_size(vec2(45.0, 45.0))
                                        .corner_radius(5.0),
                                )
                                .on_hover_text(addon.category.as_str());
                            }
                        });

                        row.col(|ui| {
                            if addon
                                .download_total
                                .as_deref()
                                .unwrap_or("0")
                                .parse::<i64>()
                                .unwrap_or(0)
                                > 5000
                            {
                                ui_show_star(ui);
                            }
                            let mut job = LayoutJob::default();
                            let format = TextFormat {
                                font_id: TextStyle::Heading.resolve(ui.style()),
                                color: ui.visuals().strong_text_color(),
                                ..Default::default()
                            };
                            job.wrap = TextWrapping {
                                max_rows: 1,
                                break_anywhere: true,
                                max_width: ui.available_width(),
                                ..Default::default()
                            };
                            job.append(&addon.name, 0.0, format);
                            if ui.selectable_label(false, job).clicked() {
                                response.addon_id = addon.id;
                                response.source =
                                    InstalledAddonSource::Esoui { esoui_id: addon.id };
                                response.response_type = AddonResponseType::AddonName;
                            }
                        });

                        row.col(|ui| {
                            ui.label(truncate_len(&addon.author_name, 15));
                        });

                        row.col(|ui| {
                            ui.label(addon.date.split(' ').next().unwrap_or(""));
                        });

                        row.col(|ui| {
                            number_cell(ui, addon.download_total.as_deref().unwrap_or("0"));
                        });

                        row.col(|ui| {
                            number_cell(ui, addon.download_monthly.as_deref().unwrap_or("0"));
                        });

                        row.col(|ui| {
                            number_cell(ui, addon.favorite_total.as_deref().unwrap_or("0"));
                        });
                    });
                });
        });
        response
    }
}

const ROW_HEIGHT: f32 = 50.0;
const ACTION_COL_WIDTH: f32 = 32.0;
const ICON_WIDTH: f32 = 50.0;
// Name collapses to this floor (~12-15 chars) before the horizontal scrollbar
// takes over
const NAME_MIN: f32 = 130.0;
// fits the 15-char truncated author name and the "Author" sort header
const AUTHOR_WIDTH: f32 = 120.0;
const ACTION_BTN: egui::Vec2 = vec2(24.0, 22.0);

// Muted pastel tones so the icons read as colored without clashing with the
// otherwise low-contrast gray palette.
const COLOR_INSTALL: Color32 = Color32::from_rgb(129, 199, 132);
const COLOR_UPDATE: Color32 = Color32::from_rgb(230, 198, 109);

fn action_button(ui: &mut egui::Ui, glyph: &str, hover: &str, color: Color32) -> bool {
    ui.add_sized(
        ACTION_BTN,
        egui::Button::new(RichText::new(glyph).heading().color(color)),
    )
    .on_hover_text(hover)
    .clicked()
}

fn right_aligned(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), contents);
}

/// Width a sortable header needs: the label plus the sort arrow that appears when
/// the column is active, plus the surrounding selectable_label padding.
fn header_width(ui: &egui::Ui, label: &str) -> f32 {
    let font = TextStyle::Heading.resolve(ui.style());
    let text = ui
        .painter()
        .layout_no_wrap(format!("{label} ⏷"), font, Color32::PLACEHOLDER)
        .size()
        .x;
    text + 2.0 * ui.spacing().button_padding.x + ui.spacing().item_spacing.x
}

/// Width a body-font cell value needs, with a little trailing margin.
fn body_width(ui: &egui::Ui, sample: &str) -> f32 {
    let font = TextStyle::Body.resolve(ui.style());
    ui.painter()
        .layout_no_wrap(sample.to_owned(), font, Color32::PLACEHOLDER)
        .size()
        .x
        + ui.spacing().item_spacing.x
}

fn number_cell(ui: &mut egui::Ui, text: &str) {
    right_aligned(ui, |ui| {
        ui.label(text);
    });
}

/// Abbreviate a numeric string to the nearest thousand (e.g. "1234" → "1K", "1500000" → "1500K").
/// Returns the original text for values under 1000 or if parsing fails.
fn abbreviate_number(text: &str) -> String {
    if let Ok(n) = text.parse::<i64>()
        && n >= 1_000
    {
        let k = n as f64 / 1_000.0;
        if k >= 1_000.0 {
            return format!("{:.0}M", k / 1_000.0);
        }
        if k.fract() == 0.0 {
            return format!("{:.0}K", k);
        }
        return format!("{:.1}K", k);
    }
    text.to_string()
}

fn sortable_header(
    ui: &mut egui::Ui,
    sort: &mut Sort,
    ascending: &mut bool,
    label: &str,
    column: Sort,
) {
    sortable_header_generic(ui, sort, ascending, label, column)
}

/// Generic sortable header that works with any sort enum implementing Display, PartialEq, and Clone.
fn sortable_header_generic<T>(
    ui: &mut egui::Ui,
    sort: &mut T,
    ascending: &mut bool,
    label: &str,
    column: T,
) where
    T: PartialEq + Clone + std::fmt::Display,
{
    let active = *sort == column;
    let text = if active {
        format!("{label} {}", if *ascending { "⏶" } else { "⏷" })
    } else {
        label.to_string()
    };
    if ui
        .selectable_label(active, RichText::new(text).heading())
        .clicked()
    {
        if active {
            *ascending = !*ascending;
        } else {
            *sort = column.clone();
            // Default ascending for non-Sort types; overridden by caller for InstalledSort.
            *ascending = true;
        }
    }
}

pub fn ui_show_star(ui: &mut egui::Ui) {
    ui.label(RichText::new("★").color(Color32::YELLOW))
        .on_hover_text("Popular! (More than 5000 downloads)");
}

// ---------------------------------------------------------------------------
// Simplified table for non-ESOUI addons (Local and Git)
// ---------------------------------------------------------------------------

/// Sort columns for installed addon tables.
#[derive(
    Debug, PartialEq, Clone, Copy, EnumIter, serde::Deserialize, serde::Serialize, Default,
)]
pub enum InstalledSort {
    Name,
    Author,
    Version,
    TotalDownloads,
    MonthlyDownloads,
    Favorites,
    #[default]
    Id,
}
impl fmt::Display for InstalledSort {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            InstalledSort::Name => write!(f, "Name"),
            InstalledSort::Author => write!(f, "Author"),
            InstalledSort::Version => write!(f, "Version"),
            InstalledSort::TotalDownloads => write!(f, "Downloads"),
            InstalledSort::MonthlyDownloads => write!(f, "Monthly"),
            InstalledSort::Favorites => write!(f, "Likes"),
            InstalledSort::Id => write!(f, "Source"),
        }
    }
}
impl InstalledSort {
    pub fn default_ascending(&self) -> bool {
        matches!(
            self,
            InstalledSort::Name
                | InstalledSort::Author
                | InstalledSort::TotalDownloads
                | InstalledSort::MonthlyDownloads
                | InstalledSort::Favorites
                | InstalledSort::Id
        )
    }
}

/// Generic row for installed addons of any source.
#[derive(Debug, Clone)]
pub struct InstalledAddonRow {
    pub id: i64,
    pub name: String,
    pub author: Option<String>,
    pub version: String,
    pub installed_version: Option<String>,
    pub is_upgradable: bool,
    /// Category string ("Addons" or "Libraries")
    pub category: String,
    /// Category icon URL from ESOUI (only ESOUI-sourced addons)
    pub category_icon: Option<String>,
    /// Source type for action buttons
    pub source: InstalledAddonSource,
    /// ESOUI addon ID if this local addon can be promoted. Only set for Local source.
    pub esoui_match_id: Option<i64>,
    /// Total downloads (for gold star display; only ESOUI addons have this)
    pub download_total: Option<String>,
    /// Monthly downloads (only ESOUI addons have this)
    pub download_monthly: Option<String>,
    /// Favorites / likes (only ESOUI addons have this)
    pub favorite_total: Option<String>,
}

impl From<AddonShowDetails> for InstalledAddonRow {
    fn from(addon: AddonShowDetails) -> Self {
        let is_upgradable = addon.is_upgradable();
        Self {
            id: addon.id,
            name: addon.name,
            author: if addon.author_name.is_empty() {
                None
            } else {
                Some(addon.author_name)
            },
            version: addon.version,
            installed_version: addon.installed_version,
            is_upgradable,
            category: addon.category,
            category_icon: addon.category_icon,
            source: InstalledAddonSource::Esoui { esoui_id: addon.id },
            esoui_match_id: None,
            download_total: addon.download_total,
            download_monthly: addon.download_monthly,
            favorite_total: addon.favorite_total,
        }
    }
}

/// Identifies where an installed addon row came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstalledAddonSource {
    /// Managed via ESOUI.com — supports Update and Remove
    Esoui { esoui_id: i64 },
    /// Managed via a Git repository — supports Pull and Remove
    Git { git_addon_id: i64, url: String },
    /// Manually installed with no remote source — supports Remove only
    Local { local_addon_id: i64 },
}

impl InstalledAddonSource {
    pub fn label(&self) -> String {
        match self {
            InstalledAddonSource::Esoui { .. } => "ESOUI".to_string(),
            InstalledAddonSource::Git { .. } => "git".to_string(),
            InstalledAddonSource::Local { .. } => "Local".to_string(),
        }
    }
}

/// Sort installed addon rows by the given column.
pub fn sort_installed_addons(
    addons: &mut [InstalledAddonRow],
    sort: InstalledSort,
    ascending: bool,
) {
    let count =
        |value: &Option<String>| value.as_deref().unwrap_or("0").parse::<i64>().unwrap_or(0);
    match sort {
        InstalledSort::Author => {
            addons.sort_by_key(|a| a.author.clone().unwrap_or_default().to_lowercase())
        }
        InstalledSort::Name => addons.sort_by_key(|a| a.name.to_lowercase()),
        InstalledSort::Version => addons.sort_by_key(|a| a.version.clone()),
        InstalledSort::TotalDownloads => addons.sort_by_key(|a| count(&a.download_total)),
        InstalledSort::MonthlyDownloads => addons.sort_by_key(|a| count(&a.download_monthly)),
        InstalledSort::Favorites => addons.sort_by_key(|a| count(&a.favorite_total)),
        InstalledSort::Id => addons.sort_by_key(|a| a.id),
    }
    if !ascending {
        addons.reverse();
    }
    // Keep upgradable addons pinned to the top regardless of sort.
    addons.sort_by_key(|a| std::cmp::Reverse(a.is_upgradable));
}

const INSTALLED_ACTION_COL_WIDTH: f32 = 32.0;
const INSTALLED_ICON_WIDTH: f32 = 50.0;
const INSTALLED_NAME_MIN: f32 = 130.0;

/// Source color palette (muted pastel tones).
const COLOR_LOCAL: Color32 = Color32::from_rgb(180, 180, 180);
const COLOR_GIT: Color32 = Color32::from_rgb(129, 199, 132);
const COLOR_ESOUI: Color32 = Color32::from_rgb(130, 160, 230);

/// Returns a color for a source type.
fn source_color(source: &InstalledAddonSource) -> Color32 {
    match source {
        InstalledAddonSource::Git { .. } => COLOR_GIT,
        InstalledAddonSource::Local { .. } => COLOR_LOCAL,
        InstalledAddonSource::Esoui { .. } => COLOR_ESOUI,
    }
}

/// Renders a source badge with a colored label.
fn source_badge(ui: &mut egui::Ui, source: &InstalledAddonSource) {
    let color = source_color(source);
    ui.label(RichText::new(source.label()).color(color).small());
}

/// Table for all installed addons regardless of source.
pub struct InstalledAddonTable<'a> {
    addons: &'a Vec<InstalledAddonRow>,
}
impl<'a> InstalledAddonTable<'a> {
    pub fn new(addons: &'a Vec<InstalledAddonRow>) -> Self {
        Self { addons }
    }

    pub fn ui(
        &self,
        ui: &mut egui::Ui,
        sort: &mut InstalledSort,
        ascending: &mut bool,
    ) -> AddonResponse {
        let num_rows = self.addons.len();
        let mut response = AddonResponse::default();

        egui::ScrollArea::horizontal().show(ui, |ui| {
            let author_width = header_width(ui, "Author").max(body_width(ui, "abcdefghijklmno"));
            let version_width = header_width(ui, "Version").max(body_width(ui, "1.2.3.4"));
            let downloads_width = header_width(ui, "Downloads").max(body_width(ui, "100,000"));
            let monthly_width = header_width(ui, "Monthly").max(body_width(ui, "10,000"));
            let likes_width = header_width(ui, "Likes").max(body_width(ui, "10,000"));
            let source_width = header_width(ui, "Source").max(body_width(ui, "Git Repo URL"));
            let spacing_x = ui.spacing().item_spacing.x;
            let fixed = INSTALLED_ACTION_COL_WIDTH
                + INSTALLED_ICON_WIDTH
                + author_width
                + version_width
                + downloads_width
                + monthly_width
                + likes_width
                + source_width
                + 6.0 * spacing_x;
            let name_width = (ui.available_width() - ui.spacing().scroll.allocated_width() - fixed)
                .max(INSTALLED_NAME_MIN);

            TableBuilder::new(ui)
                .auto_shrink(true)
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .sense(egui::Sense::hover())
                .column(Column::exact(INSTALLED_ACTION_COL_WIDTH))
                .column(Column::exact(INSTALLED_ICON_WIDTH))
                .column(Column::exact(name_width).clip(true))
                .column(Column::exact(author_width).clip(true))
                .column(Column::exact(version_width))
                .column(Column::exact(downloads_width))
                .column(Column::exact(monthly_width))
                .column(Column::exact(likes_width))
                .column(Column::exact(source_width).clip(true))
                .header(24.0, |mut header| {
                    header.col(|_| {});
                    header.col(|_| {});
                    header.col(|ui| {
                        sortable_header_generic(ui, sort, ascending, "Name", InstalledSort::Name)
                    });
                    header.col(|ui| {
                        sortable_header_generic(
                            ui,
                            sort,
                            ascending,
                            "Author",
                            InstalledSort::Author,
                        )
                    });
                    header.col(|ui| {
                        sortable_header_generic(
                            ui,
                            sort,
                            ascending,
                            "Version",
                            InstalledSort::Version,
                        )
                    });
                    header.col(|ui| {
                        right_aligned(ui, |ui| {
                            sortable_header_generic(
                                ui,
                                sort,
                                ascending,
                                "Downloads",
                                InstalledSort::TotalDownloads,
                            )
                        })
                    });
                    header.col(|ui| {
                        right_aligned(ui, |ui| {
                            sortable_header_generic(
                                ui,
                                sort,
                                ascending,
                                "Monthly",
                                InstalledSort::MonthlyDownloads,
                            )
                        })
                    });
                    header.col(|ui| {
                        right_aligned(ui, |ui| {
                            sortable_header_generic(
                                ui,
                                sort,
                                ascending,
                                "Likes",
                                InstalledSort::Favorites,
                            )
                        })
                    });
                    header.col(|ui| {
                        sortable_header_generic(ui, sort, ascending, "Source", InstalledSort::Id)
                    });
                })
                .body(|body| {
                    body.rows(ROW_HEIGHT, num_rows, |mut row| {
                        let addon = &self.addons[row.index()];

                        // Action column: update/pull/promote only
                        row.col(|ui| {
                            ui.allocate_ui_with_layout(
                                vec2(ui.available_width(), ACTION_BTN.y),
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().interact_size = vec2(0.0, 0.0);

                                    // Promote button for local addons with ESOUI match
                                    if let InstalledAddonSource::Local { .. } = &addon.source {
                                        if addon.esoui_match_id.is_some()
                                            && action_button(ui, "🔗", "Link to ESOUI", COLOR_ESOUI)
                                        {
                                            response.local_addon_id = addon.id;
                                            response.addon_id = addon.esoui_match_id.unwrap_or(0);
                                            response.response_type =
                                                AddonResponseType::PromoteLocalToEsoui;
                                        }
                                        return;
                                    }

                                    // Update button for ESOUI/Git (not Local)
                                    if addon.is_upgradable {
                                        let (update_id, update_type) = match &addon.source {
                                            InstalledAddonSource::Esoui { esoui_id } => {
                                                (*esoui_id, AddonResponseType::CheckUpdate)
                                            }
                                            InstalledAddonSource::Git { git_addon_id, .. } => {
                                                (*git_addon_id, AddonResponseType::PullGitRepo)
                                            }
                                            InstalledAddonSource::Local { .. } => return,
                                        };
                                        if action_button(ui, "⮉", "Update available", COLOR_UPDATE)
                                        {
                                            match update_type {
                                                AddonResponseType::PullGitRepo => {
                                                    response.git_repo_id = update_id;
                                                }
                                                _ => {
                                                    response.addon_id = update_id;
                                                }
                                            }
                                            response.response_type = update_type;
                                        }
                                    }
                                },
                            );
                        });

                        // Category icon column
                        row.col(|ui| {
                            if let Some(icon) = &addon.category_icon {
                                ui.add(
                                    Image::new(icon)
                                        .fit_to_exact_size(vec2(45.0, 45.0))
                                        .corner_radius(5.0),
                                )
                                .on_hover_text(addon.category.as_str());
                            }
                        });

                        // Name column
                        row.col(|ui| {
                            if addon
                                .download_total
                                .as_deref()
                                .unwrap_or("0")
                                .parse::<i64>()
                                .unwrap_or(0)
                                > 5000
                            {
                                ui_show_star(ui);
                            }
                            let mut job = LayoutJob::default();
                            let format = TextFormat {
                                font_id: TextStyle::Heading.resolve(ui.style()),
                                color: ui.visuals().strong_text_color(),
                                ..Default::default()
                            };
                            job.wrap = TextWrapping {
                                max_rows: 1,
                                break_anywhere: true,
                                max_width: ui.available_width(),
                                ..Default::default()
                            };
                            job.append(&addon.name, 0.0, format);
                            if ui.selectable_label(false, job).clicked() {
                                response.addon_id = addon.id;
                                response.response_type = AddonResponseType::AddonName;
                                response.source = addon.source.clone();
                            }
                        });

                        // Author column
                        row.col(|ui| {
                            let author = addon.author.as_deref().unwrap_or("(unknown)");
                            ui.label(truncate_len(&author.to_string(), 15));
                        });

                        // Version column
                        row.col(|ui| {
                            let version =
                                addon.installed_version.as_deref().unwrap_or(&addon.version);
                            ui.label(version);
                        });

                        // Downloads column
                        row.col(|ui| {
                            number_cell(
                                ui,
                                &abbreviate_number(addon.download_total.as_deref().unwrap_or("-")),
                            );
                        });

                        // Monthly downloads column
                        row.col(|ui| {
                            number_cell(
                                ui,
                                &abbreviate_number(
                                    addon.download_monthly.as_deref().unwrap_or("-"),
                                ),
                            );
                        });

                        // Likes column
                        row.col(|ui| {
                            number_cell(
                                ui,
                                &abbreviate_number(addon.favorite_total.as_deref().unwrap_or("-")),
                            );
                        });

                        // Source column
                        row.col(|ui| {
                            source_badge(ui, &addon.source);
                        });
                    });
                });
        });

        response
    }
}
