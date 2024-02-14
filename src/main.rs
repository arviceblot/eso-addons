use dotenv::dotenv;
use eframe::egui::{self, Label, Response, RichText, Style};
use eso_addons_core::service::AddonService;
use std::time::Duration;

mod views;
use views::addon_details::Details;
// use views::browse::Browse;
use views::installed::Installed;
use views::missing_deps::MissingDeps;
use views::onboard::Onboard;
use views::search::Search;
use views::settings::Settings;
use views::ui_helpers::{AddonResponse, AddonResponseType, PromisedValue, ViewOpt};
use views::View;

const APP_NAME: &str = "ESO Addon Manager";
pub const REPO: Option<&str> = option_env!("CARGO_PKG_REPOSITORY");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<(), eframe::Error> {
    dotenv().ok();

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([960.0, 600.0]),
        follow_system_theme: true,
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
    // browse: Browse,
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
            search: Search::new(),
            settings: Settings::default(),
            // browse: Browse::default(),
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
        // TODO: consider a view stack as views get more complicated (author view from detail from search, etc.)
        if self.view != self.prev_view {
            if self.view == ViewOpt::Installed {
                // update addons list in case any were modified from another view
                self.installed_view
                    .get_installed_addons(self.service.value.as_mut().unwrap());
                self.prev_view = self.view;
            } else if self.view == ViewOpt::Search {
                // update search results in case any were modified from another view
                self.search
                    .handle_search(self.service.value.as_mut().unwrap());
                self.prev_view = self.view;
            }
        }
    }
    fn handle_addon_selected(&mut self, addon_id: i32) {
        if self.selected_addon.is_some() && addon_id != self.selected_addon.unwrap() {
            return;
        }
        self.selected_addon = Some(addon_id);
        self.details.set_addon(
            self.selected_addon.unwrap(),
            self.service.value.as_mut().unwrap(),
        );
        self.prev_view = self.view;
        self.view = ViewOpt::Details;
    }
    fn handle_quit(&mut self) {
        self.service.value.as_mut().unwrap().save_config();
    }
}

impl eframe::App for EamApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // save config before closing
        if ctx.input(|i| i.viewport().close_requested()) {
            self.handle_quit();
        }
        // force repaint every 1 second for installs/updates
        ctx.request_repaint_after(Duration::new(1, 0));

        egui::SidePanel::left("main_left")
            .resizable(true)
            .default_width(200.0)
            .width_range(80.0..=200.0)
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    ui.add_space(5.0);
                    ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);

                    ui.selectable_value(
                        &mut self.view,
                        ViewOpt::Installed,
                        RichText::new("✔ Installed").heading(),
                    );
                    ui.selectable_value(
                        &mut self.view,
                        ViewOpt::Search,
                        RichText::new("🔍 Find More").heading(),
                    );
                    // ui.selectable_value(&mut self.view, ViewOpt::Browse, "Browse");
                    ui.selectable_value(
                        &mut self.view,
                        ViewOpt::Settings,
                        RichText::new("⛭ Settings").heading(),
                    );
                    ui.selectable_value(
                        &mut self.view,
                        ViewOpt::Quit,
                        RichText::new("⊗ Quit").heading(),
                    );
                });
            });
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

            self.check_view_update();

            let response: AddonResponse = match self.view {
                ViewOpt::Installed => {
                    self.installed_view
                        .ui(ctx, ui, self.service.value.as_mut().unwrap())
                }
                ViewOpt::Search => self
                    .search
                    .ui(ctx, ui, self.service.value.as_mut().unwrap()),
                // ViewOpt::Author => None,
                ViewOpt::Settings => {
                    self.settings
                        .ui(ctx, ui, self.service.value.as_mut().unwrap())
                }
                ViewOpt::Details => self
                    .details
                    .ui(ctx, ui, self.service.value.as_mut().unwrap()),
                ViewOpt::Quit => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    AddonResponse::default()
                }
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

            match response.response_type {
                AddonResponseType::AddonName => {
                    self.handle_addon_selected(response.addon_id);
                }
                AddonResponseType::Close => {
                    // swap back to previous view
                    if self.view == ViewOpt::Details {
                        self.selected_addon = None;
                    }
                    std::mem::swap(&mut self.prev_view, &mut self.view);
                }
                AddonResponseType::None => {}
                AddonResponseType::Update => todo!(),
                AddonResponseType::Install => todo!(),
                AddonResponseType::Remove => todo!(),
            }
            // self.handle_addon_selected(addon_id);
        });
    }
}
