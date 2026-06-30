#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use dotenv::dotenv;
use eframe::egui::{self, RichText, Visuals, vec2};
use eso_addons_core::config;
use eso_addons_core::service::AddonService;
use eso_addons_core::service::result::{AddonDepOption, AddonShowDetails, UpdateResult};
use lazy_async_promise::{ImmediateValuePromise, ImmediateValueState};
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tracing::log::info;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::writer::MakeWriterExt;
use views::author::Author;

mod views;
use views::addon_details::Details;
use views::errors::Errors;
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

    // init log file
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::WEEKLY)
        .max_log_files(5)
        .filename_prefix("eso-addons.log")
        .build(eso_addons_core::config::Config::default_config_dir())
        .expect("failed to initialize rolling file appender");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .with_ansi(false)
        .with_writer(io::stdout.and(non_blocking))
        .init();

    let icon = eframe::icon_data::from_png_bytes(&include_bytes!("../data/icon.png")[..])
        .expect("Failed to load icon");
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 600.0])
            .with_min_inner_size([800.0, 500.0]),
        // follow_system_theme: true, // as of 2024-02-19, does not work on linux. TODO: figure out if we need to move this
        ..Default::default()
    };
    options.viewport.icon = Some(Arc::new(icon));

    // create service outside app
    let service = AddonService::new().await;

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| {
            // This gives us image support:
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(EamApp::new(cc, service)))
        }),
    )
}

struct EamApp {
    /// The correct active view
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
    errors_view: Errors,
    /// Addon Service with async network/DB
    service: AddonService,
    /// Addon management promises
    remove: PromisedValue<()>,
    update_one: HashMap<i32, PromisedValue<()>>,
    install_one: HashMap<i32, PromisedValue<()>>,
    installed_addons: PromisedValue<Vec<AddonShowDetails>>,
    update: PromisedValue<UpdateResult>,
    ttc_pricetable: PromisedValue<config::TtcConfigUpdate>,
    hm_data: Option<ImmediateValuePromise<config::HmConfigUpdate>>,
    missing_deps: PromisedValue<Vec<AddonDepOption>>,
    install_missing_deps: PromisedValue<()>,
    /// Only auto-nav to MissingDeps when newly discovered, not on every refresh.
    had_missing_deps: bool,
    /// Report once the GUI is up and exit; set by ESO_ADDONS_SMOKE_TEST.
    smoke_test: bool,
    smoke_reported: bool,
}

impl EamApp {
    fn new(cc: &eframe::CreationContext<'_>, service: AddonService) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.

        // force repaint every 1 second for installs/updates
        cc.egui_ctx.request_repaint_after(Duration::new(1, 0));

        // force ppi to 1 for correct steamdeck size
        cc.egui_ctx.set_pixels_per_point(1.0);

        // set theme based on save config
        let style = match service.config.style {
            config::Style::Light => Some(Visuals::light()),
            config::Style::Dark => Some(Visuals::dark()),
            config::Style::System => None,
        };
        if let Some(visuals) = style {
            cc.egui_ctx.set_global_style(egui::Style {
                visuals,
                url_in_tooltip: true,
                ..egui::Style::default()
            });
        }

        let mut app = Self {
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
            errors_view: Errors::default(),
            remove: PromisedValue::default(),
            update_one: HashMap::new(),
            install_one: HashMap::new(),
            installed_addons: PromisedValue::default(),
            update: PromisedValue::default(),
            ttc_pricetable: PromisedValue::default(),
            hm_data: None,
            missing_deps: PromisedValue::default(),
            install_missing_deps: PromisedValue::default(),
            had_missing_deps: false,
            smoke_test: std::env::var_os("ESO_ADDONS_SMOKE_TEST").is_some(),
            smoke_reported: false,
        };
        if app.service.config.update_on_launch {
            // check for update on init
            app.check_update();
        } else {
            // check update TTC PriceTable
            if app.service.config.update_ttc_pricetable {
                app.ttc_pricetable.set(app.service.update_ttc_pricetable());
            }
            // check HarvestMap data
            if app.service.config.update_hm_data {
                app.hm_data = Some(app.service.update_hm_data());
            }
            app.get_installed_addons();
        }
        app
    }

    fn poll(&mut self) {
        // track if any addons have changed so we can notify other views
        let mut addons_changed = false;

        self.update
            .poll_recording(&self.service, "Checking for addon updates");
        if self.update.is_ready() && !self.installed_addons.is_polling() {
            self.update.handle();
            info!("Updated addon list.");
            self.get_installed_addons();
        }
        self.ttc_pricetable
            .poll_recording(&self.service, "Updating TTC PriceTable");
        if self.ttc_pricetable.is_ready() {
            info!("Updated TTC PriceTable.");
            if let Some(update) = self.ttc_pricetable.value.take() {
                self.service.config.apply_ttc_update(update);
                self.service.save_config();
            }
            self.ttc_pricetable.handle();
        }

        if let Some(mut hm_data) = self.hm_data.take() {
            match hm_data.poll_state() {
                ImmediateValueState::Updating => {
                    self.hm_data = Some(hm_data);
                }
                ImmediateValueState::Success(update) => {
                    info!("Updated HarvestMap data.");
                    self.service.config.apply_hm_update(update.clone());
                    self.service.save_config();
                }
                ImmediateValueState::Error(e) => {
                    self.service.record_error("Updating HarvestMap data", &**e);
                }
                ImmediateValueState::Empty => {}
            }
        }

        if self.update_one.is_empty() {
            self.installed_addons
                .poll_recording(&self.service, "Loading installed addons");
        }
        if self.installed_addons.is_ready() {
            self.installed_addons.handle();
            // check missing dependencies
            self.check_missing_deps();
            // force sort as addons list may have updated
            let installed = self.installed_addons.value.as_ref().unwrap();
            self.settings
                .set_installed_ids(installed.iter().map(|a| a.id).collect());
            self.installed_view = Installed::new().displayed_addons(installed.to_owned());
        }

        self.missing_deps
            .poll_recording(&self.service, "Checking missing dependencies");
        if self.missing_deps.is_ready() {
            self.missing_deps.handle();
            let deps = self.missing_deps.value.as_ref().unwrap().to_owned();
            let has_missing = !deps.is_empty();
            self.missing_dep.set_addons(
                self.installed_addons
                    .value
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(|x| (x.id, x.name.to_string()))
                    .collect(),
            );
            self.missing_dep.set_deps(deps);
            if has_missing {
                if !self.had_missing_deps {
                    self.change_view(ViewOpt::MissingDeps);
                }
            } else if self.view == ViewOpt::MissingDeps {
                self.close_view();
            }
            self.had_missing_deps = has_missing;
        }

        // poll installing missing dependencies
        self.install_missing_deps
            .poll_recording(&self.service, "Installing missing dependencies");
        if self.install_missing_deps.is_ready() {
            self.install_missing_deps.handle();
            addons_changed = true;
        }

        // remove addon poll
        self.remove.poll_recording(&self.service, "Removing addon");
        if self.remove.is_ready() {
            self.remove.handle();
            addons_changed = true;
        }

        // update addons poll
        let mut updated_addons = vec![];

        for (addon_id, promise) in self.update_one.iter_mut() {
            promise.poll_recording(&self.service, &format!("Updating addon {addon_id}"));
            if promise.is_ready() {
                updated_addons.push(addon_id.to_owned());
                promise.handle();
                info!("Updated addon: {addon_id}");
            }
        }

        for addon_id in updated_addons.iter() {
            self.update_one.remove(addon_id);
        }

        addons_changed =
            addons_changed || (!updated_addons.is_empty() && self.update_one.is_empty());

        // install addons poll
        let mut installed_addons = vec![];
        for (addon_id, promise) in self.install_one.iter_mut() {
            promise.poll_recording(&self.service, &format!("Installing addon {addon_id}"));
            if promise.is_ready() {
                installed_addons.push(addon_id.to_owned());
                promise.handle();
                addons_changed = true;
            }
        }
        for addon_id in installed_addons.iter() {
            self.install_one.remove(addon_id);
        }

        if addons_changed {
            self.handle_addons_changed();
        }
    }

    // region: View Management

    fn handle_addon_selected(&mut self, addon_id: i32) {
        self.details.set_addon(addon_id, &mut self.service);
        if self.view != ViewOpt::Details {
            self.change_view(ViewOpt::Details);
        }
    }

    fn change_view(&mut self, view: ViewOpt) {
        self.view_stack.push(self.view);
        self.prev_view = self.view;
        self.view = view;
    }

    fn close_view(&mut self) {
        let Some(view) = self.view_stack.pop() else {
            return;
        };
        if view == ViewOpt::Root {
            self.view_stack.push(view);
            return;
        }
        self.view = view;
    }

    // endregion

    // region: AddOn Management

    fn handle_addons_changed(&mut self) {
        // update installed addons
        self.get_installed_addons();
        // update views
        self.search.reset(&mut self.service);
        self.details.reset(&mut self.service);
        self.author_view.reset(&mut self.service);
    }

    fn remove_addon(&mut self, addon_id: i32) {
        let mut promise = PromisedValue::<()>::default();
        promise.set(self.service.remove(addon_id));
        self.remove = promise;
    }

    fn update_addon(&mut self, addon_id: i32) {
        let mut promise = PromisedValue::<()>::default();
        promise.set(self.service.install(addon_id, true));
        self.update_one.insert(addon_id, promise);
    }

    fn install_addon(&mut self, addon_id: i32) {
        let mut promise = PromisedValue::<()>::default();
        promise.set(self.service.install(addon_id, false));
        self.install_one.insert(addon_id, promise);
    }

    /// Check for updates but do not upgrade any addons
    fn check_update(&mut self) {
        info!("Checking for updates");
        self.update.set(self.service.update(false));
        // check update TTC PriceTable
        if self.service.config.update_ttc_pricetable {
            self.ttc_pricetable
                .set(self.service.update_ttc_pricetable());
        }
        // check HarvestMap data
        if self.service.config.update_hm_data {
            self.hm_data = Some(self.service.update_hm_data());
        }
    }

    fn get_installed_addons(&mut self) {
        if self.installed_addons.is_polling() {
            return;
        }
        info!("Getting installed addons");
        self.installed_addons
            .set(self.service.get_installed_addons());
    }

    /// Check for missing dependencies
    fn check_missing_deps(&mut self) {
        self.missing_deps
            .set(self.service.get_missing_dependency_options());
    }

    // endregion

    // region: Context Handlers

    fn handle_quit(&mut self) {
        self.service.save_config();
    }

    // endregion
}

impl eframe::App for EamApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;

        if self.smoke_test {
            if !self.smoke_reported {
                use std::io::Write;
                println!("eso-addons: smoke test ok");
                let _ = io::stdout().flush();
                self.smoke_reported = true;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            self.handle_quit();
        }

        if ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Extra1)) {
            self.close_view();
        }

        self.poll();

        // if we are loading addons, show spinner and that's it
        if self.update.is_polling() || self.installed_addons.is_polling() {
            egui::CentralPanel::default().show(ui, |ui| {
                // ui.vertical_centered_justified(|ui| {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                });
                // })
            });
            return;
        }

        // check if need onboarding
        if self.service.config.onboard {
            egui::CentralPanel::default().show(ui, |ui| {
                self.onboard.ui(ctx, ui, &mut self.service);
            });
            return;
        }

        egui::Panel::left("main_left")
            .resizable(true)
            .default_size(200.0)
            .size_range(80.0..=200.0)
            .show(ui, |ui| {
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
                    let error_count = self.service.errors().len();
                    if error_count > 0 {
                        ui.selectable_value(
                            &mut self.view,
                            ViewOpt::Errors,
                            RichText::new(format!("⚠ Errors ({error_count})")).heading(),
                        );
                    }
                    ui.selectable_value(
                        &mut self.view,
                        ViewOpt::Quit,
                        RichText::new("⊗ Quit").heading(),
                    );
                });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    // show active progress items
                    if self.update.is_polling() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Checking for updates");
                        });
                    }
                    if self.ttc_pricetable.is_polling() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Updating TTC PriceTable");
                        });
                    }
                    if self.hm_data.is_some() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Updating HarvestMap data");
                        });
                    }
                    let installing_count =
                        self.install_one.values().filter(|x| x.is_polling()).count();
                    if installing_count > 0 {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(format!("Installing {installing_count} addons"));
                        });
                    }
                    let updating_count =
                        self.update_one.values().filter(|x| x.is_polling()).count();
                    if updating_count > 0 {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(format!("Updating {updating_count} addons"));
                        });
                    }
                });
            });
        egui::CentralPanel::default().show(ui, |ui| {
            // check if need onboarding
            if self.service.config.onboard {
                self.onboard.ui(ctx, ui, &mut self.service);
                return;
            }

            let response: AddonResponse = match self.view {
                ViewOpt::Installed => self.installed_view.ui(ctx, ui, &mut self.service),
                ViewOpt::Search => self.search.ui(ctx, ui, &mut self.service),
                ViewOpt::Settings => self.settings.ui(ctx, ui, &mut self.service),
                ViewOpt::Details => self.details.ui(ctx, ui, &mut self.service),
                ViewOpt::Quit => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    AddonResponse::default()
                }
                ViewOpt::Author => self.author_view.ui(ctx, ui, &mut self.service),
                ViewOpt::MissingDeps => self.missing_dep.ui(ctx, ui, &mut self.service),
                ViewOpt::Errors => self.errors_view.ui(ctx, ui, &mut self.service),
                ViewOpt::Root => {
                    self.view = ViewOpt::Installed;
                    AddonResponse::default()
                }
            };

            match response.response_type {
                AddonResponseType::AddonName => {
                    self.handle_addon_selected(response.addon_id);
                }
                AddonResponseType::AuthorName => {
                    self.author_view
                        .author_name(response.author_name, &self.service);
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
