use bbcode_tagger::{BBNode, BBTag, BBTree};
use std::fmt;
use tracing::log::error;

use eframe::{
    egui::{self, RichText, TextFormat},
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
    Installed,
    Search,
    Browse,
    Settings,
    Details,
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

pub fn ui_show_addon_item(ui: &mut egui::Ui, addon: &AddonShowDetails) -> Option<i32> {
    // col1:
    // addon_name, author
    // category
    let mut return_id = None;
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(false, RichText::new(addon.name.as_str()).strong())
                .clicked()
            {
                return_id = Some(addon.id);
            }
            ui.label(RichText::new(format!("by: {}", addon.author_name.as_str())).small());
        });
        ui.label(RichText::new(addon.category.as_str()).small());
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
                    egui::Label::new(format!(
                        "⮋ {}",
                        addon.download_total.as_ref().unwrap().as_str()
                    ))
                    .wrap(false),
                );
            }
            // "♥" favorites
            if addon.favorite_total.is_some() {
                ui.add(
                    egui::Label::new(format!(
                        "♥ {}",
                        addon.favorite_total.as_ref().unwrap().as_str()
                    ))
                    .wrap(false),
                );
            }
            // "🔃" version
            ui.add(egui::Label::new(format!("🔃 {}", addon.version)).wrap(false));
        }
    });
    return_id
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
fn ui_handle_text(ui: &mut egui::Ui, node: &BBNode, i: i32, parent_nodes: &[BBNode]) {
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
        // if *tag == BBTag::Bold {
        //     text.strong();
        // } else if *tag == BBTag::Italic {
        //     text.italics();
        // }
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
            BBTag::FontColor => {}
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
            _ => {} //Ok(text),
        };
    }
    job.append(node.text.as_str(), 0.0, text_fmt);
    ui.label(job);
}
