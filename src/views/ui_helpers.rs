use bbcode_tagger::{BBNode, BBTag, BBTree};
use itertools::any;
use std::fmt;
use tracing::log::error;

use eframe::{
    egui::{self, Label, Response, RichText, TextFormat, Widget},
    emath::Align,
    epaint::{text::LayoutJob, Color32, FontId, Stroke},
};
use eso_addons_core::service::result::AddonShowDetails;
use lazy_async_promise::{ImmediateValuePromise, ImmediateValueState};
use strum_macros::EnumIter;

#[derive(Debug, PartialEq, Clone, Copy, EnumIter)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum Sort {
    Name,
    Updated,
    Author,
    TotalDownloads,
    MonthlyDownloads,
    Favorites,
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
impl Default for Sort {
    fn default() -> Self {
        Self::Id
    }
}

#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewOpt {
    /// Not really a reachable view, but a base
    Root,
    // Onboard,
    // MissingDep,
    Installed,
    Search,
    // Author,
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
    pub fn new(value_promise: ImmediateValuePromise<T>) -> Self {
        Self {
            promise: Some(value_promise),
            value: None,
            handled: false,
        }
    }

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
                error!("{}", format!("Error fetching data: {}", **e));
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

pub fn truncate(text: &String) -> String {
    if text.len() > 60 {
        let mut new_text = text[..60].to_string();
        new_text.push_str(" ...");
        return new_text;
    }
    text.to_string()
}

use egui_extras::{Column, TableBuilder, TableRow};

pub enum AddonResponseType {
    None,
    AddonName,
    Update,
    Install,
    Remove,
    Close,
}
pub struct AddonResponse {
    pub addon_id: i32,
    pub response_type: AddonResponseType,
    pub source: Option<ViewOpt>,
}
impl Default for AddonResponse {
    fn default() -> Self {
        Self {
            addon_id: 0,
            response_type: AddonResponseType::None,
            source: None,
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
            // .striped(true)
            // .resizable(self.resizable)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .sense(egui::Sense::hover())
            .column(Column::remainder())
            .column(Column::auto())
            .column(Column::auto())
            .body(|body| {
                body.rows(80.0, num_rows, |mut row| {
                    let addon = &addons[row.index()];

                    // col1:
                    // addon_name
                    // author
                    // category
                    row.col(|ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(
                                        false,
                                        RichText::new(truncate(&addon.name)).heading().strong(),
                                    )
                                    .clicked()
                                {
                                    response.addon_id = addon.id;
                                    response.response_type = AddonResponseType::AddonName;
                                }
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
                            ui.label(RichText::new(format!("by: {}", addon.author_name.as_str())));
                            ui.label(RichText::new(addon.category.as_str()));
                        });
                    });

                    // col2:
                    // download total
                    // favorites
                    // version
                    row.col(|ui| {
                        ui.vertical(|ui| {
                            let default = String::new();
                            let installed_version =
                                addon.installed_version.as_ref().unwrap_or(&default);
                            if addon.is_upgradable() {
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new(addon.version.as_str()).color(Color32::GREEN),
                                    );
                                    ui.label(installed_version);
                                });
                            } else {
                                if addon.download_total.is_some() {
                                    // "⮋" downloads
                                    ui.add(
                                        Label::new(format!(
                                            "⮋ {}",
                                            addon.download_total.as_ref().unwrap().as_str()
                                        ))
                                        .wrap(false),
                                    );
                                }
                                // "♥" favorites
                                if addon.favorite_total.is_some() {
                                    ui.add(
                                        Label::new(format!(
                                            "♥ {}",
                                            addon.favorite_total.as_ref().unwrap().as_str()
                                        ))
                                        .wrap(false),
                                    );
                                }
                                // "🔃" version
                                ui.add(Label::new(format!("🔃 {}", addon.version)).wrap(false));
                            }
                        });
                    });

                    // col3: install/update button?
                    row.col(|ui| {
                        if allow_install {
                            ui.horizontal_centered(|ui| {
                                if !addon.installed
                                    && ui.button(RichText::new("Install").heading()).clicked()
                                {
                                    response.addon_id = addon.id;
                                    response.response_type = AddonResponseType::Install;
                                } else if addon.installed && addon.is_upgradable() {
                                    // if self.is_updating_addon(addon.id) {
                                    // ui.centered_and_justified(|ui| {
                                    //     ui.add_enabled(
                                    //         false,
                                    //         egui::Button::new("Updating..."),
                                    //     );
                                    // });
                                    // } else if ui.button("Update").clicked() {
                                    if ui.button(RichText::new("Update").heading()).clicked() {
                                        response.addon_id = addon.id;
                                        response.response_type = AddonResponseType::Update;
                                    }
                                }
                            });
                        }
                    });
                });
            });
        response
    }
}

// pub struct AddonRow<'a> {
//     addon: &'a AddonShowDetails,
// }
// impl AddonRow {
//     pub fn new(addon: &AddonShowDetails) -> Self {
//         Self { addon }
//     }
//     pub fn ui(&self, row: &mut TableRow<'_, '_>) {
//         let Self { addon } = self;
//         row.col(|ui| {
//             ui.label(addon.name.as_str());
//         });
//     }
// }

pub fn ui_show_star(ui: &mut egui::Ui) {
    ui.label(RichText::new("★").color(Color32::YELLOW))
        .on_hover_text("Popular! (More than 5000 downloads)");
}

pub fn ui_show_addon_item(ui: &mut egui::Ui, addon: &AddonShowDetails) -> Option<Response> {
    // col1:
    // addon_name, author
    // category
    let mut response = None;
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            response = Some(ui.selectable_label(false, addon.name.as_str()));
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
        ui.label(RichText::new(format!("by: {}", addon.author_name.as_str())));
        ui.label(RichText::new(addon.category.as_str()));
    });
    // col2:
    // download total
    // favorites
    // version
    ui.vertical(|ui| {
        let default = String::new();
        let installed_version = addon.installed_version.as_ref().unwrap_or(&default);
        if addon.is_upgradable() {
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(addon.version.as_str()).color(Color32::GREEN));
                ui.label(installed_version);
            });
        } else {
            if addon.download_total.is_some() {
                // "⮋" downloads
                ui.add(
                    Label::new(format!(
                        "⮋ {}",
                        addon.download_total.as_ref().unwrap().as_str()
                    ))
                    .wrap(false),
                );
            }
            // "♥" favorites
            if addon.favorite_total.is_some() {
                ui.add(
                    Label::new(format!(
                        "♥ {}",
                        addon.favorite_total.as_ref().unwrap().as_str()
                    ))
                    .wrap(false),
                );
            }
            // "🔃" version
            ui.add(Label::new(format!("🔃 {}", addon.version)).wrap(false));
        }
    });
    response
}

pub fn ui_show_bbtree(ui: &mut egui::Ui, tree: &BBTree) {
    ui.horizontal_wrapped(|ui| {
        ui_show_bbnode(ui, tree, 0, &mut vec![]);
    });
}
fn ui_show_bbnode(ui: &mut egui::Ui, tree: &BBTree, i: i32, parent_nodes: &mut Vec<BBNode>) {
    let node = tree.get_node(i);
    let text = node.text.as_str();

    // TODO: take in to account the parent tags

    let mut children_handled = false;

    match node.tag {
        BBTag::None => {
            ui.label(node.text.as_str());
        }
        BBTag::Bold
        | BBTag::Italic
        | BBTag::Underline
        | BBTag::Strikethrough
        | BBTag::FontSize
        | BBTag::FontColor
        | BBTag::Center
        | BBTag::Left
        | BBTag::Right
        | BBTag::Superscript
        | BBTag::Subscript
        | BBTag::ListItem => {
            ui_handle_text(ui, node, i, parent_nodes.as_ref());
        }
        BBTag::Quote => {
            ui.label(text);
        }
        BBTag::Spoiler => {
            ui.label(text);
        }
        BBTag::Link => {
            // no URL to create link, use text
            let mut value = text;
            if node.value.is_some() {
                value = node.value.as_ref().unwrap().as_str();
            }
            if text.is_empty() {
                ui.hyperlink(value);
            } else {
                ui.hyperlink_to(text, value);
            }
        }
        BBTag::Image => {
            ui.label(text);
        }
        BBTag::ListOrdered => {
            children_handled = true;
            let id = ui.make_persistent_id(format!("{}_list", i));
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
                .show_header(ui, |ui| {
                    ui.label(node.text.as_str());
                })
                .body(|ui| {
                    ui.vertical(|ui| {
                        for (index, node) in node.children.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("{index}.) "));
                                ui_show_bbnode(ui, tree, *node, parent_nodes);
                            });
                        }
                    });
                });
        }
        BBTag::ListUnordered => {
            children_handled = true;
            ui.vertical(|ui| {
                ui.style_mut().wrap = Some(true);
                if let Some(title) = &node.value {
                    if !title.is_empty() {
                        ui.label(node.text.as_str());
                    }
                }
                for node in node.children.iter() {
                    ui.horizontal(|ui| {
                        ui.label("– ");
                        ui_show_bbnode(ui, tree, *node, parent_nodes);
                    });
                }
            });
        } // "–"
        BBTag::Code => {
            ui.label(RichText::new(text).code());
        }
        BBTag::Preformatted => {
            ui.label(text);
        }
        BBTag::Table => {
            ui.label(text);
        }
        BBTag::TableHeading => {
            ui.label(text);
        }
        BBTag::TableRow => {
            ui.label(text);
        }
        BBTag::TableCell => {
            ui.label(text);
        }
        BBTag::YouTube => {
            ui.label(text);
        }
        BBTag::Blur => {
            ui.label(text);
        }
        BBTag::Email => {
            ui.label(text);
        }
        BBTag::Unknown => {
            ui.label(text);
        }
    };

    if children_handled {
        return;
    }
    parent_nodes.push(node.clone());
    for child in node.children.iter() {
        ui_show_bbnode(ui, tree, *child, parent_nodes);
    }
    parent_nodes.pop();
}
fn ui_handle_text(ui: &mut egui::Ui, node: &BBNode, _i: i32, parent_nodes: &[BBNode]) {
    // skip empty text
    if node.text.trim().is_empty() {
        return;
    }
    let mut job = LayoutJob::default();
    let mut text_fmt = TextFormat::default();

    let (default_color, strong_color) = if ui.visuals().dark_mode {
        (Color32::LIGHT_GRAY, Color32::WHITE)
    } else {
        (Color32::DARK_GRAY, Color32::BLACK)
    };
    text_fmt.color = default_color;

    // tag on the current node to apply the same formatting
    for n in parent_nodes.iter().chain([node]) {
        match n.tag {
            BBTag::Bold => {
                text_fmt.color = strong_color;
            }
            BBTag::Italic => {
                text_fmt.italics = true;
            }
            BBTag::Underline => {
                text_fmt.underline = Stroke::new(1.0, text_fmt.color);
            }
            BBTag::Strikethrough => {
                text_fmt.strikethrough = Stroke::new(1.0, text_fmt.color);
            }
            BBTag::FontColor => {
                // if n.value.is_some_and(|x| x.starts_with("#")) {
                // let text_color = node.value.unwrap().as_str();
                // text_fmt.color = hex_color!(text_color);
                // }
            }
            BBTag::FontSize => {
                if let Some(size) = &node.value {
                    match size.as_str() {
                        "1" => text_fmt.font_id = FontId::proportional(32.0),
                        "2" => text_fmt.font_id = FontId::proportional(24.0),
                        "3" => text_fmt.font_id = FontId::proportional(20.8),
                        "4" => text_fmt.font_id = FontId::proportional(16.0),
                        "5" => text_fmt.font_id = FontId::proportional(12.8),
                        "6" => text_fmt.font_id = FontId::proportional(11.2),
                        _ => {}
                    }
                }
            }
            BBTag::Center => {
                text_fmt.valign = Align::Center;
            }
            BBTag::Left => {
                text_fmt.valign = Align::LEFT;
            }
            BBTag::Right => text_fmt.valign = Align::RIGHT,
            BBTag::Superscript => {
                text_fmt.font_id = FontId::proportional(7.0);
                text_fmt.valign = Align::TOP;
            }
            BBTag::Subscript => {
                text_fmt.font_id = FontId::proportional(7.0);
                text_fmt.valign = Align::BOTTOM;
            }
            _ => {}
        };
    }
    job.append(node.text.as_str(), 0.0, text_fmt);
    ui.label(job);
}
