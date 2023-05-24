use dotenv::dotenv;
use eframe::egui;
use eso_addons_core::service::AddonService;
use views::View;

mod views;

use views::addon_details::Details;
use views::browse::Browse;
use views::installed::Installed;
use views::missing_deps::MissingDeps;
use views::onboard::Onboard;
use views::search::Search;
use views::settings::Settings;
use views::ui_helpers::{PromisedValue, ViewOpt};

const APP_NAME: &str = "ESO Addon Manager";
pub const REPO: Option<&str> = option_env!("CARGO_PKG_REPOSITORY");

#[tokio::main]
async fn main() -> Result<(), eframe::Error> {
    dotenv().ok();

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .init();

    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(960.0, 600.0)),
        ..Default::default()
    };
    eframe::run_native(APP_NAME, options, Box::new(|_cc| Box::<EamApp>::default()))
}

struct EamApp {
    view: ViewOpt,
    prev_view: ViewOpt,
    installed_view: Installed,
    search: Search,
    settings: Settings,
    browse: Browse,
    service: PromisedValue<AddonService>,
    selected_addon: Option<i32>,
    details: Details,
    onboard: Onboard,
    missing_dep: MissingDeps,
}
impl Default for EamApp {
    fn default() -> Self {
        let mut service = PromisedValue::<AddonService>::default();
        service.set(AddonService::new());

        EamApp {
            view: ViewOpt::Installed,
            prev_view: ViewOpt::Installed,
            installed_view: Installed::new(),
            search: Search::default(),
            settings: Settings::default(),
            browse: Browse::default(),
            service,
            selected_addon: None,
            details: Details::default(),
            onboard: Onboard::default(),
            missing_dep: MissingDeps::new(),
        }
    }
}

impl EamApp {
    fn check_view_update(&mut self) {
        if self.view != self.prev_view {
            if self.view == ViewOpt::Installed {
                // update addons list in case any were modified from another view
                self.installed_view
                    .get_installed_addons(self.service.value.as_mut().unwrap());
            } else if self.view == ViewOpt::Search {
                // update search results in case any were modified from another view
                self.search
                    .handle_search(self.service.value.as_mut().unwrap());
            }
        }
        self.prev_view = self.view;
    }
    fn handle_addon_selected(&mut self, addon_id: Option<i32>) {
        if let Some(addon_id) = addon_id {
            if self.selected_addon.is_some() && addon_id != self.selected_addon.unwrap() {
                return;
            }
            self.selected_addon = Some(addon_id);
            self.details.set_addon(
                self.selected_addon.unwrap(),
                self.service.value.as_mut().unwrap(),
            );
        }
    }
}

impl eframe::App for EamApp {
    fn on_close_event(&mut self) -> bool {
        self.service.value.as_mut().unwrap().save_config();
        true
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.service.is_ready() {
                self.service.poll();
                ui.spinner();
                return;
            }

            // check if need onboarding
            if self.service.value.as_ref().unwrap().config.onboard {
                self.onboard
                    .ui(ctx, ui, self.service.value.as_mut().unwrap());
                return;
            }

            // check if missing deps
            if self.missing_dep.has_missing() {
                self.missing_dep
                    .ui(ctx, ui, self.service.value.as_mut().unwrap());
                return;
            }

            if self.selected_addon.is_some() {
                // show addon details view
                if ui.button("Close").clicked() {
                    self.selected_addon = None;
                    return;
                }
                self.details
                    .ui(ctx, ui, self.service.value.as_mut().unwrap());
                return;
            }

            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.view, ViewOpt::Installed, "Installed");
                ui.selectable_value(&mut self.view, ViewOpt::Search, "Search");
                ui.selectable_value(&mut self.view, ViewOpt::Browse, "Browse");
                ui.selectable_value(&mut self.view, ViewOpt::Settings, "Settings");
            });
            ui.separator();
            self.check_view_update();

            let addon_id: Option<i32> = match self.view {
                ViewOpt::Installed => {
                    self.installed_view
                        .ui(ctx, ui, self.service.value.as_mut().unwrap())
                }
                ViewOpt::Search => self
                    .search
                    .ui(ctx, ui, self.service.value.as_mut().unwrap()),
                ViewOpt::Browse => self
                    .browse
                    .ui(ctx, ui, self.service.value.as_mut().unwrap()),
                ViewOpt::Settings => {
                    self.settings
                        .ui(ctx, ui, self.service.value.as_mut().unwrap())
                }
                ViewOpt::Details => None,
            };

            // check missing dep from update result
            if self.installed_view.update.value.is_some() {
                let missing = &self
                    .installed_view
                    .update
                    .value
                    .as_ref()
                    .unwrap()
                    .missing_deps;
                if !missing.is_empty() {
                    self.missing_dep.set_deps(missing.to_vec());
                    self.installed_view
                        .update
                        .value
                        .as_mut()
                        .unwrap()
                        .missing_deps
                        .clear();
                }
            }
            self.handle_addon_selected(addon_id);
        });
    }
}
