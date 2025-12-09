use std::fmt;
use tracing::log::error;

use eframe::{
    egui::{
        self, Image, RichText, TextFormat, TextStyle,
        text::{LayoutJob, TextWrapping},
        vec2,
    },
    epaint::Color32,
};
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
            Sort::Updated => write!(f, "Updated"),
            Sort::Author => write!(f, "Author"),
            Sort::TotalDownloads => write!(f, "Total Downloads"),
            Sort::MonthlyDownloads => write!(f, "Monthly Downloads"),
            Sort::Favorites => write!(f, "Favorites"),
            Sort::Id => write!(f, "ID"),
        }
    }
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
    Quit,
}

#[derive(Default)]
pub struct PromisedValue<T: Send + Clone + Default + 'static> {
    promise: Option<ImmediateValuePromise<T>>,
    pub value: Option<T>,
    handled: bool,
}
impl<T: Send + Clone + Default> PromisedValue<T> {
    pub fn poll(&mut self) {
        if self.promise.is_none() {
            return;
        }
        let state = self.promise.as_mut().unwrap().poll_state();
        // TODO: Strongly consider saving error here if not in progress or success
        match state {
            ImmediateValueState::Success(state) => {
                self.value = Some(state.clone()); // copy out of promise
                self.promise = None;
            }
            ImmediateValueState::Error(e) => {
                error!("Error fetching data: {}", **e);
                self.promise = None;
            }
            _ => {}
        }
        // if let ImmediateValueState::Success(val) = state {
        //     self.value = Some(val.clone()); // copy out of promise
        //     self.promise = None;
        // }
    }
    pub fn set(&mut self, value_promise: ImmediateValuePromise<T>) {
        self.promise = Some(value_promise);
        self.value = None;
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
    pub fn ui(&self, ui: &mut egui::Ui) -> AddonResponse {
        let Self {
            addons,
            allow_install,
        } = *self;
        // let has_updateable = any(addons.iter(), |x| x.is_upgradable());
        let num_rows = addons.len();
        let mut response = AddonResponse::default();
        TableBuilder::new(ui)
            .auto_shrink(true)
            .striped(true)
            // .resizable(self.resizable)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .sense(egui::Sense::hover())
            .max_scroll_height(3200.0)
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::remainder().clip(true))
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .header(24.0, |mut header| {
                header.col(|_| {});
                header.col(|_| {});
                header.col(|ui| {
                    ui.heading("Name");
                });
                header.col(|ui| {
                    ui.heading("Version");
                });
                header.col(|ui| {
                    ui.heading("Author");
                });
                header.col(|ui| {
                    ui.heading("Downloads");
                });
                header.col(|ui| {
                    ui.heading("Favorites");
                });
            })
            .body(|body| {
                body.rows(50.0, num_rows, |mut row| {
                    let addon = &addons[row.index()];

                    row.col(|ui| {
                        if allow_install {
                            if !addon.installed
                                && ui.button(RichText::new("Install").heading()).clicked()
                            {
                                response.addon_id = addon.id;
                                response.response_type = AddonResponseType::Install;
                            } else if addon.installed
                                && addon.is_upgradable()
                                && ui.button(RichText::new("Update").heading()).clicked()
                            {
                                response.addon_id = addon.id;
                                response.response_type = AddonResponseType::Update;
                            }
                        }
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
                            .as_ref()
                            .unwrap()
                            .parse::<i32>()
                            .unwrap()
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
                            ..Default::default()
                        };
                        job.append(&addon.name, 0.0, format);
                        if ui.selectable_label(false, job).clicked() {
                            response.addon_id = addon.id;
                            response.response_type = AddonResponseType::AddonName;
                        }
                    });

                    row.col(|ui| {
                        ui.label(truncate_len(&addon.version, 10));
                    });

                    row.col(|ui| {
                        ui.label(truncate_len(&addon.author_name, 15));
                    });

                    row.col(|ui| {
                        ui.label(addon.download_total.as_ref().unwrap().as_str());
                    });

                    row.col(|ui| {
                        ui.label(addon.favorite_total.as_ref().unwrap().as_str());
                    });
                });
            });
        response
    }
}

pub fn ui_show_star(ui: &mut egui::Ui) {
    ui.label(RichText::new("★").color(Color32::YELLOW))
        .on_hover_text("Popular! (More than 5000 downloads)");
}
