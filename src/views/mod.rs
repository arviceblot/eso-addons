use eframe::egui;
use eso_addons_core::service::AddonService;
use tokio::runtime::Handle;
use tokio::runtime::Runtime;

// pub mod browse;
pub mod installed;
// pub mod search;
pub mod settings;
pub mod ui_helpers;

pub trait View {
    fn ui(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, service: &mut AddonService);
}
