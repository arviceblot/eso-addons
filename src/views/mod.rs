use eframe::egui;
use eso_addons_core::service::AddonService;

use self::ui_helpers::AddonResponse;

pub mod addon_details;
pub mod author;
pub mod installed;
pub mod missing_deps;
pub mod onboard;
pub mod search;
pub mod settings;
pub mod ui_helpers;

pub trait View {
    fn ui(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        service: &mut AddonService,
    ) -> AddonResponse;
}

pub trait ResetView {
    fn reset(&mut self, service: &mut AddonService);
}
