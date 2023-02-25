use eframe::egui;
use eso_addons_core::service::result::AddonShowDetails;
use eso_addons_core::service::{result::ParentCategory, AddonService};
use tokio::runtime::Runtime;

use super::ui_helpers::ui_show_addon_item;
use super::View;

#[derive(Default)]
pub struct Browse {
    is_init: bool,
    parent_categories: Vec<ParentCategory>,
    displayed_addons: Vec<AddonShowDetails>,
    selected_category: i32,
    previous_category: i32,
}
impl Browse {
    fn handle_init(&mut self, rt: &Runtime, service: &AddonService) {
        if !self.is_init {
            self.parent_categories = rt.block_on(service.get_category_parents()).unwrap();
            self.is_init = true;
            self.selected_category = 0;
            self.previous_category = 0;
            self.get_addons(rt, service);
        }
    }
    fn get_addons(&mut self, rt: &Runtime, service: &AddonService) {
        self.displayed_addons = rt
            .block_on(service.get_addons_by_category(self.selected_category))
            .unwrap();
    }
    fn install_addon(&self, addon_id: i32, rt: &Runtime, service: &mut AddonService) {
        rt.block_on(service.install(addon_id, false)).unwrap();
    }
}
impl View for Browse {
    fn ui(
        &mut self,
        ctx: &eframe::egui::Context,
        ui: &mut eframe::egui::Ui,
        rt: &tokio::runtime::Runtime,
        service: &mut AddonService,
    ) {
        self.handle_init(rt, service);

        egui::SidePanel::left("left_panel")
            .resizable(true)
            .default_width(200.0)
            .width_range(80.0..=200.0)
            .show_inside(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("Categories");
                });
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::CollapsingHeader::new("All")
                        .default_open(true)
                        .show(ui, |ui| {
                            for (i, parent) in self.parent_categories.iter().enumerate() {
                                egui::CollapsingHeader::new(parent.title.as_str())
                                    .default_open(i == 0)
                                    .show(ui, |ui| {
                                        for category in parent.child_categories.iter() {
                                            if ui
                                                .add(egui::SelectableLabel::new(
                                                    false,
                                                    format!(
                                                        "{} ({})",
                                                        category.title,
                                                        category.file_count.unwrap_or(0)
                                                    ),
                                                ))
                                                .clicked()
                                            {
                                                self.selected_category = category.id;
                                            }
                                        }
                                    });
                            }
                        });
                });
            });

        if self.selected_category != self.previous_category {
            self.get_addons(rt, service);
            self.previous_category = self.selected_category;
        }
        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut installed = false;
                egui::Grid::new("addon_grid")
                    .striped(true)
                    .spacing([5.0, 20.0])
                    .show(ui, |ui| {
                        // only show not-installed addons in search results
                        for addon in self.displayed_addons.iter() {
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
                    self.get_addons(rt, service);
                }
            });
        });
    }
}
