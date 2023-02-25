use eframe::egui;
use eso_addons_core::service::AddonService;
use tokio::runtime::{self, Runtime};
use views::View;

mod views;

use views::browse::Browse;
use views::installed::Installed;
use views::search::Search;
use views::settings::Settings;
use views::ui_helpers::ViewOpt;

const APP_NAME: &str = "ESO Addon Manager";
pub const REPO: Option<&str> = option_env!("CARGO_PKG_REPOSITORY");

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(960.0, 600.0)),
        ..Default::default()
    };
    eframe::run_native(APP_NAME, options, Box::new(|_cc| Box::new(EamApp::new())))
}

struct EamApp {
    view: ViewOpt,
    installed_view: Installed,
    search: Search,
    settings: Settings,
    browse: Browse,
    rt: Runtime,
    service: AddonService,
}

impl EamApp {
    pub fn new() -> EamApp {
        let rt = runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let service = rt.block_on(AddonService::new());
        EamApp {
            view: ViewOpt::Installed,
            installed_view: Installed::new(),
            search: Search::new(),
            settings: Settings::default(),
            browse: Browse::default(),
            rt,
            service,
        }
    }
}

impl eframe::App for EamApp {
    fn on_close_event(&mut self) -> bool {
        self.service.save_config();
        true
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.view, ViewOpt::Installed, "Installed");
                ui.selectable_value(&mut self.view, ViewOpt::Search, "Search");
                ui.selectable_value(&mut self.view, ViewOpt::Browse, "Browse");
                ui.selectable_value(&mut self.view, ViewOpt::Settings, "Settings");
            });
            ui.separator();

            match self.view {
                ViewOpt::Installed => {
                    self.installed_view
                        .get_installed_addons(&self.rt, &mut self.service);
                    self.installed_view.ui(ctx, ui, &self.rt, &mut self.service);
                }
                ViewOpt::Search => {
                    self.search.ui(ctx, ui, &self.rt, &mut self.service);
                }
                ViewOpt::Browse => {
                    self.browse.ui(ctx, ui, &self.rt, &mut self.service);
                }
                ViewOpt::Settings => self.settings.ui(ctx, ui, &self.rt, &mut self.service),
            }
        });
    }
}
