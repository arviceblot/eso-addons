use eframe::egui::{self, RichText, ScrollArea};
use eframe::epaint::Color32;
use eso_addons_core::service::result::{AddonShowDetails, SearchDbAddon};
use eso_addons_core::service::AddonService;
use std::fmt;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;
use tokio::runtime::{self, Runtime};

const APP_NAME: &str = "ESO Addon Manager";
const REPO: Option<&str> = option_env!("CARGO_PKG_REPOSITORY");

#[derive(Debug, PartialEq, Clone, Copy, EnumIter)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
enum Sort {
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
enum ViewOpt {
    Installed,
    Search,
    Browse,
}

struct EamApp {
    view: ViewOpt,
    installed_view: Installed,
    search: Search,
}

impl EamApp {
    pub fn new() -> EamApp {
        EamApp {
            view: ViewOpt::Installed,
            installed_view: Installed::new(),
            search: Search::new(),
        }
    }
}

impl eframe::App for EamApp {
    fn on_close_event(&mut self) -> bool {
        self.installed_view.service.save_config();
        true
    }
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    // if ui.button("Save").clicked() {
                    //     // TODO: Add functionality
                    // }
                    if ui.button("Quit").clicked() {
                        frame.close();
                    }
                });
                ui.menu_button("Settings", |ui| {
                    ui.checkbox(
                        &mut self
                            .installed_view
                            .service
                            .config
                            .update_on_launch
                            .get_or_insert(false),
                        "Update on launch",
                    );
                    ui.checkbox(
                        &mut self
                            .installed_view
                            .service
                            .config
                            .update_ttc_pricetable
                            .get_or_insert(false),
                        "Update TTC PriceTable",
                    );
                });
                ui.menu_button("Help", |ui| {
                    // if ui.button("Logs").clicked() {
                    //     // TODO: Add functionality
                    // }
                    // if ui.button("About").clicked() {
                    //     // TODO: Add functionality
                    // }
                    if REPO.is_some() {
                        ui.hyperlink_to("GitHub", REPO.unwrap());
                    }
                })
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.view, ViewOpt::Installed, "Installed");
                ui.selectable_value(&mut self.view, ViewOpt::Search, "Search");
                ui.selectable_value(&mut self.view, ViewOpt::Browse, "Browse");
            });
            ui.separator();

            match self.view {
                ViewOpt::Installed => {
                    self.installed_view.get_installed_addons();
                    self.installed_view.ui(ui);
                }
                ViewOpt::Search => {
                    self.search.ui(ui);
                }
                ViewOpt::Browse => {
                    // TODO:
                }
            }
        });
    }
}

struct Installed {
    installed_addons: Vec<AddonShowDetails>,
    addons_updated: Vec<String>,
    filter: String,
    sort: Sort,
    prev_sort: Sort,
    init: bool,
    editing: bool,
    rt: Runtime,
    service: AddonService,
}
pub trait View {
    fn ui(&mut self, ui: &mut egui::Ui);
}

impl Installed {
    pub fn new() -> Installed {
        let rt = runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let service = rt.block_on(AddonService::new());
        Installed {
            installed_addons: vec![],
            addons_updated: vec![],
            filter: Default::default(),
            sort: Sort::Name,
            prev_sort: Sort::Name,
            init: true,
            editing: false,
            rt,
            service,
        }
    }
    fn show_init(&mut self) -> bool {
        let init = self.init;
        if self.init {
            self.init = false;
        }
        init
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

        if self.service.config.update_ttc_pricetable.unwrap_or(false) {
            self.rt
                .block_on(self.service.update_ttc_pricetable())
                .unwrap();
            self.addons_updated
                .push("TTC PriceTable Updated!".to_string());
        }
    }
    fn get_installed_addons(&mut self) {
        let result = self
            .rt
            .block_on(self.service.get_installed_addons())
            .unwrap();
        self.installed_addons = result;
        self.sort_addons();
    }
    fn handle_sort(&mut self) {
        if self.prev_sort != self.sort {
            self.prev_sort = self.sort;
            self.sort_addons();
        }
    }
    fn sort_addons(&mut self) {
        match self.sort {
            Sort::Author => self.installed_addons.sort_by(|a, b| {
                a.author_name
                    .to_lowercase()
                    .cmp(&b.author_name.to_lowercase())
            }),
            Sort::Name => self
                .installed_addons
                .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            Sort::Updated => self.installed_addons.sort_by(|a, b| a.date.cmp(&b.date)),
            Sort::TotalDownloads => {
                // TODO:
            }
            Sort::MonthlyDownloads => {
                // TODO:
            }
            Sort::Favorites => {
                // TODO:
            }
            Sort::Id => self.installed_addons.sort_by(|a, b| a.id.cmp(&b.id)),
        }
    }
    fn remove_addon(&self, addon_id: i32) {
        self.rt.block_on(self.service.remove(addon_id)).unwrap();
    }
}
impl View for Installed {
    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.show_init() {
            // TODO: move blocking install count out of update loop!
            if self.service.config.update_on_launch.unwrap_or(false) {
                self.update_addons();
            }
            self.get_installed_addons();
        }

        if self.installed_addons.is_empty() {
            ui.label("No addons installed!");
        } else {
            self.handle_sort();
            ui.horizontal(|ui| {
                if ui.button("Update All").clicked() {
                    // TODO: move blocking update out of update loop!
                    self.update_addons();
                }
                egui::ComboBox::from_id_source("sort")
                    .selected_text(format!("Sort By: {}", self.sort.to_string().to_uppercase()))
                    .show_ui(ui, |ui| {
                        ui.style_mut().wrap = Some(false);
                        ui.set_min_width(60.0);
                        for sort in Sort::iter() {
                            ui.selectable_value(&mut self.sort, sort, sort.to_string());
                        }
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .desired_width(120.0)
                        .hint_text("Filter..."),
                );
                self.filter = self.filter.to_lowercase();
                if ui.button("🗙").clicked() {
                    self.filter.clear();
                }
            });
            ui.horizontal(|ui| {
                ui.label(format!("Installed: {}", self.installed_addons.len()));
                ui.checkbox(&mut self.editing, "Edit");
            });
            ui.separator();
            ui.vertical_centered_justified(|ui| {
                ScrollArea::vertical()
                    .max_height(200.0)
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            let mut remove_id: Option<i32> = Default::default();
                            egui::Grid::new("addon_grid")
                                .striped(true)
                                .spacing([5.0, 20.0])
                                .show(ui, |ui| {
                                    for addon in self.installed_addons.iter() {
                                        // col0 x button if editing
                                        if self.editing {
                                            ui.horizontal_centered(|ui| {
                                                if ui
                                                    .button(RichText::new("🗙").color(Color32::RED))
                                                    .clicked()
                                                {
                                                    remove_id = Some(addon.id);
                                                }
                                            });
                                        }
                                        // col1:
                                        // addon_name, author
                                        // category
                                        ui.vertical(|ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(addon.name.as_str()).strong(),
                                                );
                                                ui.label(
                                                    RichText::new(format!(
                                                        "by: {}",
                                                        addon.author_name.as_str()
                                                    ))
                                                    .small(),
                                                );
                                            });
                                            ui.label(
                                                RichText::new(addon.category.as_str()).small(),
                                            );
                                        });
                                        // col2:
                                        // download total
                                        // favorites
                                        // version
                                        ui.vertical(|ui| {
                                            if addon.download_total.is_some() {
                                                // "⮋" downloads
                                                ui.add(
                                                    egui::Label::new(format!(
                                                        "⮋ {}",
                                                        addon
                                                            .download_total
                                                            .as_ref()
                                                            .unwrap()
                                                            .as_str()
                                                    ))
                                                    .wrap(false),
                                                );
                                            }
                                            // "♥" favorites
                                            if addon.favorite_total.is_some() {
                                                ui.add(
                                                    egui::Label::new(format!(
                                                        "♥ {}",
                                                        addon
                                                            .favorite_total
                                                            .as_ref()
                                                            .unwrap()
                                                            .as_str()
                                                    ))
                                                    .wrap(false),
                                                );
                                            }
                                            // "🔃" version
                                            ui.add(
                                                egui::Label::new(format!("🔃 {}", addon.version))
                                                    .wrap(false),
                                            );
                                        });
                                        ui.end_row();
                                    }
                                });
                            if remove_id.is_some() {
                                self.remove_addon(remove_id.unwrap());
                                self.get_installed_addons();
                            }
                        });
                    });
            });
            ui.separator();
        }
        // log scroll area
        ui.collapsing("Log", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ScrollArea::vertical().max_height(20.0).show(ui, |ui| {
                    ui.vertical(|ui| {
                        for update in self.addons_updated.iter() {
                            ui.label(update);
                        }
                    });
                });
            });
        });
    }
}

struct Search {
    results: Vec<SearchDbAddon>,
    search: String,
    rt: Runtime,
    service: AddonService,
}
impl Search {
    pub fn new() -> Search {
        let rt = runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let service = rt.block_on(AddonService::new());
        Search {
            results: vec![],
            search: Default::default(),
            rt,
            service,
        }
    }

    fn handle_search(&mut self) {
        self.search = self.search.to_lowercase();
        let results = self.rt.block_on(self.service.search(&self.search)).unwrap();
        self.results = results;
    }

    fn install_addon(&self, addon_id: i32) {
        self.rt
            .block_on(self.service.install(addon_id, false))
            .unwrap();
    }
}
impl View for Search {
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search"));
            if ui.button("Search").clicked() {
                self.handle_search();
            }
        });
        ui.separator();

        ui.vertical_centered_justified(|ui| {
            ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        let mut installed = false;
                        for result in self.results.iter() {
                            ui.horizontal(|ui| {
                                if !result.installed && ui.button("+").clicked() {
                                    self.install_addon(result.id);
                                    installed = true;
                                }
                                ui.label(result.name.as_str());
                            });
                        }
                        if installed {
                            self.handle_search();
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
