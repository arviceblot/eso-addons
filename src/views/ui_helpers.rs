use std::fmt;

use eframe::{
    egui::{self, RichText},
    epaint::Color32,
};
use eso_addons_core::service::result::AddonShowDetails;
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

#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewOpt {
    Installed,
    Search,
    Browse,
    Settings,
}

pub fn ui_show_addon_item(ui: &mut egui::Ui, addon: &AddonShowDetails) {
    // col1:
    // addon_name, author
    // category
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(addon.name.as_str()).strong());
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
}
