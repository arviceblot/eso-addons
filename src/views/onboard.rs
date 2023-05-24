use eframe::egui;
use egui_file::FileDialog;

use crate::views::View;

#[derive(Default)]
pub struct Onboard {
    open_addon_dir_dialog: Option<FileDialog>,
    addon_dir_set: bool,
    setup_done: bool,
}
impl Onboard {
    pub fn is_setup_done(&self) -> bool {
        self.addon_dir_set
    }
}
impl View for Onboard {
    fn ui(
        &mut self,
        ctx: &eframe::egui::Context,
        ui: &mut eframe::egui::Ui,
        service: &mut eso_addons_core::service::AddonService,
    ) -> Option<i32> {
        // welcome
        ui.label("Welcome to the Unofficial ESO AddOn Manager!");
        ui.label("Let's start by finding the right foler to save your AddOns:");
        // select game addon path
        if ui.button("Select ESO AddOn folder...").clicked() {
            let mut dialog = FileDialog::select_folder(Some(service.config.addon_dir.clone()));
            dialog.open();
            self.open_addon_dir_dialog = Some(dialog);
        }
        if let Some(dialog) = &mut self.open_addon_dir_dialog {
            if dialog.show(ctx).selected() {
                if let Some(dir) = dialog.path() {
                    service.config.addon_dir = dir;
                    service.save_config();
                    self.addon_dir_set = true;
                }
            }
        }
        // import from minion option
        // done?
        if ui
            .add_enabled(self.is_setup_done(), egui::Button::new("Done!"))
            .clicked()
        {
            self.setup_done = true;
            service.config.onboard = false;
            service.save_config();
        }
        None
    }
}
