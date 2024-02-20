use dotenv::dotenv;
use eframe::egui::{self, vec2, RichText, Visuals};
use eso_addons_core::config;
use eso_addons_core::service::result::{AddonDepOption, AddonShowDetails, UpdateResult};
use eso_addons_core::service::AddonService;
use std::collections::HashMap;
use std::time::Duration;
use tracing::log::info;
use views::author::Author;

mod views;
use views::addon_details::Details;
use views::installed::Installed;
use views::missing_deps::MissingDeps;
use views::onboard::Onboard;
use views::search::Search;
use views::settings::Settings;
use views::ui_helpers::{AddonResponse, AddonResponseType, PromisedValue, ViewOpt};
use views::{ResetView, View};

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
        follow_system_theme: true, // as of 2024-02-19, does not work on linux
        ..Default::default()
    };

    // TODO: consider moving service outside UI for lifetime?
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| {
            // This gives us image support:
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Box::new(EamApp::new(cc))
        }),
    )
}

struct EamApp {
    /// The currect active view
    view: ViewOpt,
    /// Previous view, stored really only to compare against view change
    prev_view: ViewOpt,
    /// View history, probably assumes first item is ViewOpt::Root
    view_stack: Vec<ViewOpt>,
    /// Views
    installed_view: Installed,
    search: Search,
    settings: Settings,
    details: Details,
    onboard: Onboard,
    missing_dep: MissingDeps,
    author_view: Author,
    /// Addon Service with async network/DB
    service: PromisedValue<AddonService>,
    /// Addon management promises
    remove: PromisedValue<()>,
    update_one: HashMap<i32, PromisedValue<()>>,
    install_one: HashMap<i32, PromisedValue<()>>,
    installed_addons: PromisedValue<Vec<AddonShowDetails>>,
    update: PromisedValue<UpdateResult>,
    ttc_pricetable: PromisedValue<()>,
    hm_data: PromisedValue<()>,
    missing_deps: PromisedValue<Vec<AddonDepOption>>,
    install_missing_deps: PromisedValue<()>,
    // Log subscriber
}
impl Default for EamApp {
    fn default() -> Self {
        let mut service = PromisedValue::<AddonService>::default();
        service.set(AddonService::new());

        EamApp {
            view: ViewOpt::Installed,
            prev_view: ViewOpt::Root,
            view_stack: vec![ViewOpt::Root],
            installed_view: Installed::new(),
            search: Search::new(),
            settings: Settings::default(),
            service,
            details: Details::default(),
            onboard: Onboard::default(),
            missing_dep: MissingDeps::new(),
            author_view: Author::default(),
            remove: PromisedValue::default(),
            update_one: HashMap::new(),
            install_one: HashMap::new(),
            installed_addons: PromisedValue::default(),
            update: PromisedValue::default(),
            ttc_pricetable: PromisedValue::default(),
            hm_data: PromisedValue::default(),
            missing_deps: PromisedValue::default(),
            install_missing_deps: PromisedValue::default(),
        }
    }
}

impl EamApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.

        // force repaint every 1 second for installs/updates
        cc.egui_ctx.request_repaint_after(Duration::new(1, 0));

        Self::default()
    }

    fn poll(&mut self, ctx: &egui::Context) {
        // track if any addons have changed so we can notify other views
        let mut addons_changed = false;

        self.service.poll();
        if self.service.is_ready() {
            self.service.handle();
            self.check_update();

            // update style based on config on load
            match self.service.value.as_ref().unwrap().config.style {
                config::Style::Light => {
                    ctx.style_mut(|style| {
                        style.visuals = Visuals::light();
                    });
                }
                config::Style::Dark => {
                    ctx.style_mut(|style| {
                        style.visuals = Visuals::dark();
                    });
                }
                config::Style::System => {
                    // nothing to do, this is default
                }
            }
        }

        self.update.poll();
        if self.update.is_ready() && !self.installed_addons.is_polling() {
            self.update.handle();
            info!("Updated addon list.");
            self.get_installed_addons();
            self.check_missing_deps();
        }
        self.ttc_pricetable.poll();
        if self.ttc_pricetable.is_ready() {
            info!("Updated TTC PriceTable.");
            self.ttc_pricetable.handle();
        }
        self.hm_data.poll();
        if self.hm_data.is_ready() {
            info!("Updated HarvestMap data.");
            self.hm_data.handle();
        }

        self.installed_addons.poll();
        if self.installed_addons.is_ready() {
            self.installed_addons.handle();
            // force sort as addons list may have updated
            self.installed_view = Installed::new()
                .displayed_addons(self.installed_addons.value.as_ref().unwrap().to_owned());
        }

        self.missing_deps.poll();
        if self.missing_deps.is_ready() {
            self.missing_deps.handle();
            if !self.missing_deps.value.as_ref().unwrap().is_empty() {
                // we need to resolve missing dependencies
                self.missing_dep
                    .set_deps(self.missing_deps.value.as_ref().unwrap().to_owned());
                self.change_view(ViewOpt::MissingDeps);
            }
        }

        // poll installing missing dependencies
        self.install_missing_deps.poll();
        if self.install_missing_deps.is_ready() {
            self.install_missing_deps.handle();
            addons_changed = true;
        }

        // remove addon poll
        self.remove.poll();
        if self.remove.is_ready() {
            self.remove.handle();
            addons_changed = true;
        }

        // update addons poll
        let mut updated_addons = vec![];
        for (addon_id, promise) in self.update_one.iter_mut() {
            promise.poll();
            if promise.is_ready() {
                updated_addons.push(addon_id.to_owned());
                promise.handle();
                addons_changed = true;
                info!("Updated addon: {addon_id}");
            }
        }
        // let fetch_addons = !updated_addons.is_empty();
        for addon_id in updated_addons.iter() {
            self.update_one.remove(addon_id);
        }

        // install addons poll
        let mut installed_addons = vec![];
        for (addon_id, promise) in self.install_one.iter_mut() {
            promise.poll();
            if promise.is_ready() {
                installed_addons.push(addon_id.to_owned());
                promise.handle();
                addons_changed = true;
            }
        }
        // let fetch_addons = !installed_addons.is_empty();
        for addon_id in installed_addons.iter() {
            self.install_one.remove(addon_id);
        }

        if addons_changed {
            self.handle_addons_changed();
        }
    }

    // region: View Management

    fn handle_addon_selected(&mut self, addon_id: i32) {
        if self.view == ViewOpt::Details {
            return;
        }
        self.details
            .set_addon(addon_id, self.service.value.as_mut().unwrap());
        self.change_view(ViewOpt::Details);
    }

    fn change_view(&mut self, view: ViewOpt) {
        // TODO: consider if we should allow view change from important things like missing deps or onboarding
        self.view_stack.push(self.view);
        self.prev_view = self.view;
        self.view = view;
    }

    fn close_view(&mut self) {
        // swap back to previous view
        self.view = self.view_stack.pop().unwrap();
    }

    // endregion

    // region: AddOn Management

    fn handle_addons_changed(&mut self) {
        // update installed addons
        self.get_installed_addons();
        // check for missing dependencies
        self.check_missing_deps();
        // update views
        self.search.reset(self.service.value.as_mut().unwrap());
        self.details.reset(self.service.value.as_mut().unwrap());
        self.author_view.reset(self.service.value.as_mut().unwrap());
    }

    fn remove_addon(&mut self, addon_id: i32) {
        let mut promise = PromisedValue::<()>::default();
        promise.set(self.service.value.as_mut().unwrap().remove(addon_id));
        self.remove = promise;
    }

    fn update_addon(&mut self, addon_id: i32) {
        let mut promise = PromisedValue::<()>::default();
        promise.set(self.service.value.as_mut().unwrap().install(addon_id, true));
        self.update_one.insert(addon_id, promise);
    }

    fn install_addon(&mut self, addon_id: i32) {
        let mut promise = PromisedValue::<()>::default();
        promise.set(
            self.service
                .value
                .as_mut()
                .unwrap()
                .install(addon_id, false),
        );
        self.install_one.insert(addon_id, promise);
    }

    /// Check for updates but do not upgrade any addons
    fn check_update(&mut self) {
        info!("Checking for updates");
        self.update
            .set(self.service.value.as_mut().unwrap().update(false));
        // check update TTC PriceTable
        if self
            .service
            .value
            .as_mut()
            .unwrap()
            .config
            .update_ttc_pricetable
        {
            self.ttc_pricetable
                .set(self.service.value.as_mut().unwrap().update_ttc_pricetable());
        }
        // check HarvestMap data
        if self.service.value.as_mut().unwrap().config.update_hm_data {
            self.hm_data
                .set(self.service.value.as_mut().unwrap().update_hm_data());
        }
    }

    fn get_installed_addons(&mut self) {
        if self.installed_addons.is_polling() {
            return;
        }
        info!("Getting installed addons");
        self.installed_addons
            .set(self.service.value.as_mut().unwrap().get_installed_addons());
    }

    /// Check for missing dependencies
    fn check_missing_deps(&mut self) {
        self.missing_deps.set(
            self.service
                .value
                .as_mut()
                .unwrap()
                .get_missing_dependency_options(),
        );
    }

    // endregion

    // region: Context Handlers

    fn handle_quit(&mut self) {
        self.service.value.as_mut().unwrap().save_config();
    }

    // endregion
}

impl eframe::App for EamApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // gracefully handle app quit
        if ctx.input(|i| i.viewport().close_requested()) {
            self.handle_quit();
        }

        self.poll(ctx);

        // check if sevice is ready before anything else!
        if !self.service.is_ready() {
            egui::CentralPanel::default().show(ctx, |ui| {
                // ui.vertical_centered_justified(|ui| {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                });
                // })
            });
            return;
        }

        // if we are loading addons, show spinner and that's it
        if self.update.is_polling() || self.installed_addons.is_polling() {
            egui::CentralPanel::default().show(ctx, |ui| {
                // ui.vertical_centered_justified(|ui| {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                });
                // })
            });
            return;
        }

        // check if need onboarding
        if self.service.value.as_ref().unwrap().config.onboard {
            egui::CentralPanel::default().show(ctx, |ui| {
                self.onboard
                    .ui(ctx, ui, self.service.value.as_mut().unwrap());
            });
            return;
        }

        egui::SidePanel::left("main_left")
            .resizable(true)
            .default_width(200.0)
            .width_range(80.0..=200.0)
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    ui.add_space(5.0);
                    ui.spacing_mut().item_spacing = vec2(10.0, 10.0);

                    ui.selectable_value(
                        &mut self.view,
                        ViewOpt::Installed,
                        RichText::new("✔ Installed").heading(),
                    );
                    if self
                        .missing_deps
                        .value
                        .as_ref()
                        .is_some_and(|x| !x.is_empty())
                    {
                        ui.selectable_value(
                            &mut self.view,
                            ViewOpt::MissingDeps,
                            RichText::new("❗ Missing Dependencies").heading(),
                        );
                    }
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
            // check if need onboarding
            if self.service.value.as_ref().unwrap().config.onboard {
                self.onboard
                    .ui(ctx, ui, self.service.value.as_mut().unwrap());
                return;
            }

            let response: AddonResponse = match self.view {
                ViewOpt::Installed => {
                    self.installed_view
                        .ui(ctx, ui, self.service.value.as_mut().unwrap())
                }
                ViewOpt::Search => self
                    .search
                    .ui(ctx, ui, self.service.value.as_mut().unwrap()),
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
                ViewOpt::Author => {
                    self.author_view
                        .ui(ctx, ui, self.service.value.as_mut().unwrap())
                }
                ViewOpt::MissingDeps => {
                    self.missing_dep
                        .ui(ctx, ui, self.service.value.as_mut().unwrap())
                }
                ViewOpt::Root => {
                    // should not be reachable with defaults
                    todo!();
                }
            };

            match response.response_type {
                AddonResponseType::AddonName => {
                    self.handle_addon_selected(response.addon_id);
                }
                AddonResponseType::AuthorName => {
                    self.author_view
                        .author_name(response.author_name, self.service.value.as_mut().unwrap());
                    self.change_view(ViewOpt::Author);
                }
                AddonResponseType::CheckUpdate => {
                    self.check_update();
                }
                AddonResponseType::Close => {
                    self.close_view();
                }
                AddonResponseType::InstallMissingDeps => {
                    self.install_missing_deps.set(
                        self.service
                            .value
                            .as_mut()
                            .unwrap()
                            .install_missing_dependencies(response.missing_deps),
                    );
                    // this should only return from missing dep view, close it
                    self.close_view();
                }
                AddonResponseType::Update => {
                    self.update_addon(response.addon_id);
                }
                AddonResponseType::UpdateMultiple => {
                    for addon_id in response.addon_ids {
                        self.update_addon(addon_id);
                    }
                }
                AddonResponseType::Install => {
                    self.install_addon(response.addon_id);
                }
                AddonResponseType::Remove => {
                    self.remove_addon(response.addon_id);
                }
                AddonResponseType::AddonsChanged => {
                    self.handle_addons_changed();
                }
                AddonResponseType::None => {}
            }
        });
    }
}
