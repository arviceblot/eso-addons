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
use lazy_async_promise::{ImmediateValuePromise, ImmediateValueState};
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
        |value: &Option<String>| value.as_deref().unwrap_or("0").parse::<i32>().unwrap_or(0);
    match sort {
        Sort::Author => addons.sort_by(|a, b| {
            a.author_name
                .to_lowercase()
                .cmp(&b.author_name.to_lowercase())
        }),
        Sort::Name => addons.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        Sort::Updated => addons.sort_by(|a, b| a.date.cmp(&b.date)),
        Sort::TotalDownloads => {
            addons.sort_by(|a, b| count(&a.download_total).cmp(&count(&b.download_total)))
        }
        Sort::MonthlyDownloads => {
            addons.sort_by(|a, b| count(&a.download_monthly).cmp(&count(&b.download_monthly)))
        }
        Sort::Favorites => {
            addons.sort_by(|a, b| count(&a.favorite_total).cmp(&count(&b.favorite_total)))
        }
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
pub struct PromisedValue<T: Send + Clone + Default + 'static> {
    promise: Option<ImmediateValuePromise<T>>,
    pub value: Option<T>,
    error: Option<String>,
    handled: bool,
}
impl<T: Send + Clone + Default> PromisedValue<T> {
    pub fn poll(&mut self) {
        if self.promise.is_none() {
            return;
        }
        let state = self.promise.as_mut().unwrap().poll_state();
        match state {
            ImmediateValueState::Success(state) => {
                self.value = Some(state.clone());
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
    Close,
}
pub struct AddonResponse {
    pub addon_id: i32,
    pub addon_ids: Vec<i32>,
    pub author_name: String,
    pub response_type: AddonResponseType,
    pub missing_deps: Vec<MissingDepView>,
}
impl Default for AddonResponse {
    fn default() -> Self {
        Self {
            addon_id: 0,
            addon_ids: vec![],
            response_type: AddonResponseType::default(),
            author_name: "".to_string(),
            missing_deps: vec![],
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
                            let stacked = addon.installed && addon.is_upgradable();
                            let height = if stacked {
                                ACTION_BTN.y * 2.0 + ACTION_GAP
                            } else {
                                ACTION_BTN.y
                            };
                            // Fixed-size allocation so the cell's layout centers the
                            // button stack both vertically and horizontally.
                            ui.allocate_ui_with_layout(
                                vec2(ui.available_width(), height),
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().item_spacing.y = ACTION_GAP;
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
                                    if action_button(ui, "🗙", "Remove", COLOR_REMOVE) {
                                        response.addon_id = addon.id;
                                        response.response_type = AddonResponseType::Remove;
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
                                .parse::<i32>()
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
const ACTION_GAP: f32 = 2.0;

// Muted pastel tones so the icons read as colored without clashing with the
// otherwise low-contrast gray palette.
const COLOR_INSTALL: Color32 = Color32::from_rgb(129, 199, 132);
const COLOR_UPDATE: Color32 = Color32::from_rgb(230, 198, 109);
const COLOR_REMOVE: Color32 = Color32::from_rgb(229, 130, 130);

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

fn sortable_header(
    ui: &mut egui::Ui,
    sort: &mut Sort,
    ascending: &mut bool,
    label: &str,
    column: Sort,
) {
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
            *sort = column;
            *ascending = column.default_ascending();
        }
    }
}

pub fn ui_show_star(ui: &mut egui::Ui) {
    ui.label(RichText::new("★").color(Color32::YELLOW))
        .on_hover_text("Popular! (More than 5000 downloads)");
}
