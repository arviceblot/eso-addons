use eframe::egui::{self, ScrollArea};
use eso_addons_core::service::result::AddonDetails;
use eso_addons_core::service::AddonService;
use tokio::runtime;

const APP_NAME: &str = "ESO Addon Manager";

struct EamApp {
    rt: runtime::Runtime,
    service: AddonService,
    init: bool,
    installed_count: i32,
    addons_updated: Vec<String>,
}

impl EamApp {
    pub fn new() -> EamApp {
        let rt = runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let service = rt.block_on(AddonService::new());

        EamApp {
            rt,
            init: true,
            installed_count: 0,
            service,
            addons_updated: vec![],
        }
    }

    fn show_init(&mut self) -> bool {
        let init = self.init;
        if self.init {
            self.init = false;
        }
        init
    }
    fn get_installed_addon_count(&mut self) {
        self.installed_count = self
            .rt
            .block_on(self.service.get_installed_addon_count())
            .unwrap();
    }
    fn update_addons(&mut self) {
        let result = self.rt.block_on(self.service.update()).unwrap();
        for update in result.addons_updated.iter() {
            self.addons_updated
                .push(format!("{} updated!", update.name));
        }
        if result.addons_updated.is_empty() {
            self.addons_updated
                .push("Everything up to date!".to_string());
        }
    }
}

impl eframe::App for EamApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.show_init() {
                // TODO: move blocking install count out of update loop!
                self.get_installed_addon_count();
            }
            ui.horizontal(|ui| {
                ui.label(format!("Installed: {}", self.installed_count));
            });
            if ui.button("Update").clicked() {
                // TODO: move blocking update out of update loop!
                self.update_addons();
            }

            // update log scroll area
            let scroll_area = ScrollArea::vertical()
                .max_height(200.0)
                .auto_shrink([false; 2]);
            scroll_area.show(ui, |ui| {
                ui.vertical(|ui| {
                    for update in self.addons_updated.iter() {
                        ui.label(update);
                    }
                });
            });
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(600.0, 400.0)),
        ..Default::default()
    };
    eframe::run_native(APP_NAME, options, Box::new(|_cc| Box::new(EamApp::new())))
}
