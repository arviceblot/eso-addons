use eframe::egui::{self, ScrollArea};
use eso_addons_core::service::result::SearchDbAddon;
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
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Save").clicked() {
                        // TODO: Add functionality
                    }
                    if ui.button("Quit").clicked() {
                        frame.close();
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("Logs").clicked() {
                        // TODO: Add functionality
                    }
                    if ui.button("About").clicked() {
                        // TODO: Add functionality
                    }
                    if REPO.is_some() {
                        ui.hyperlink_to("Source on GitHub", REPO.unwrap());
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
                ViewOpt::Browse => todo!(),
            }
        });
    }
}

struct Installed {
    installed_addons: Vec<SearchDbAddon>,
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
            Sort::Author => {
                // TODO:
            }
            Sort::Name => self
                .installed_addons
                .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            Sort::Updated => {
                // TODO: add date to SearchDbAddon to use this sort
            }
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
            self.get_installed_addons();
        }

        if self.installed_addons.is_empty() {
            ui.label("No addons installed!");
        } else {
            self.handle_sort();
            ui.horizontal(|ui| {
                if ui.button("Update").clicked() {
                    // TODO: move blocking update out of update loop!
                    self.update_addons();
                }
                ui.checkbox(&mut self.editing, "Edit");
            });
            ui.label(format!("Installed: {}", self.installed_addons.len()));
            ui.horizontal(|ui| {
                ui.label("Filter:");
                ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(120.0));
                self.filter = self.filter.to_lowercase();
                if ui.button("ｘ").clicked() {
                    self.filter.clear();
                }
                egui::ComboBox::from_label("Sort")
                    .selected_text(format!("{}", self.sort))
                    .show_ui(ui, |ui| {
                        ui.style_mut().wrap = Some(false);
                        ui.set_min_width(60.0);
                        for sort in Sort::iter() {
                            ui.selectable_value(&mut self.sort, sort, sort.to_string());
                        }
                    });
            });
            ui.separator();
            ui.vertical_centered_justified(|ui| {
                ScrollArea::vertical()
                    .max_height(200.0)
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            let mut remove_id: Option<i32> = Default::default();
                            for addon in self.installed_addons.iter() {
                                ui.horizontal(|ui| {
                                    if self.editing && ui.button("-").clicked() {
                                        remove_id = Some(addon.id);
                                    }
                                    ui.label(addon.name.as_str());
                                });
                            }
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
