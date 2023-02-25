use super::{ui_helpers::ui_show_addon_item, View};
use eframe::egui::{self, ScrollArea};
use eso_addons_core::service::{result::AddonShowDetails, AddonService};
use tokio::runtime::Runtime;

pub struct Search {
    results: Vec<AddonShowDetails>,
    search: String,
}
impl Search {
    pub fn new() -> Search {
        Search {
            results: vec![],
            search: Default::default(),
        }
    }

    fn handle_search(&mut self, rt: &Runtime, service: &mut AddonService) {
        self.search = self.search.to_lowercase();
        let results = rt.block_on(service.search(&self.search)).unwrap();
        self.results = results;
    }

    fn install_addon(&self, addon_id: i32, rt: &Runtime, service: &mut AddonService) {
        rt.block_on(service.install(addon_id, false)).unwrap();
    }
}
impl View for Search {
    fn ui(
        &mut self,
        _ctx: &egui::Context,
        ui: &mut egui::Ui,
        rt: &Runtime,
        service: &mut AddonService,
    ) {
        ui.horizontal(|ui| {
            let response = ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search"));
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.handle_search(rt, service);
            }
            if ui.button("Search").clicked() {
                self.handle_search(rt, service);
            }
            if ui.button("Clear").clicked() {
                self.search.clear();
                self.results.clear();
            }
        });
        ui.separator();

        if !self.results.is_empty() {
            ui.horizontal(|ui| {
                ui.label(format!("Results: {}", self.results.len()));
            });
        }

        ui.vertical_centered_justified(|ui| {
            ScrollArea::vertical()
                .max_height(300.0)
                .auto_shrink([true; 2])
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        let mut installed = false;
                        egui::Grid::new("addon_grid")
                            .striped(true)
                            .spacing([5.0, 20.0])
                            .show(ui, |ui| {
                                // only show not-installed addons in search results
                                for addon in self.results.iter().filter(|x| !x.installed) {
                                    ui_show_addon_item(ui, addon);
                                    ui.horizontal_centered(|ui| {
                                        if !addon.installed && ui.button("Install").clicked() {
                                            self.install_addon(addon.id, rt, service);
                                            installed = true;
                                        }
                                    });
                                    ui.end_row();
                                }
                            });
                        if installed {
                            self.handle_search(rt, service);
                        }
                    });
                });
        });
    }
}
