use eframe::egui;
use eso_addons_core::service::result::{AddonShowDetails, ParentCategory};
use eso_addons_core::service::AddonService;

use super::ui_helpers::{ui_show_addon_item, PromisedValue};
use super::View;

#[derive(Default)]
pub struct Browse {
    is_init: bool,
    parent_categories: PromisedValue<Vec<ParentCategory>>,
    displayed_addons: PromisedValue<Vec<AddonShowDetails>>,
    selected_category: i32,
    previous_category: i32,
}
impl Browse {
    fn handle_init(&mut self, service: &AddonService) {
        if !self.is_init {
            self.parent_categories.set(service.get_category_parents());
            self.is_init = true;
            self.selected_category = 0;
            self.previous_category = 0;
            self.get_addons(service);
        }
    }
    fn poll(&mut self) {
        self.parent_categories.poll();
        if self.parent_categories.is_ready() {
            self.parent_categories.handle();
        }

        self.displayed_addons.poll();
        if self.displayed_addons.is_ready() {
            self.displayed_addons.handle();
        }
    }
    fn get_addons(&mut self, service: &AddonService) {
        self.displayed_addons
            .set(service.get_addons_by_category(self.selected_category));
    }
    fn install_addon(&self, addon_id: i32, service: &mut AddonService) {
        // TODO: add back
        // rt.block_on(service.install(addon_id, false)).unwrap();
    }
}
impl View for Browse {
    fn ui(
        &mut self,
        ctx: &eframe::egui::Context,
        ui: &mut eframe::egui::Ui,
        service: &mut AddonService,
    ) -> Option<i32> {
        self.handle_init(service);
        self.poll();

        if self.parent_categories.is_polling() {
            ui.spinner();
            return None;
        }

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
                            for (i, parent) in self
                                .parent_categories
                                .value
                                .as_ref()
                                .unwrap()
                                .iter()
                                .enumerate()
                            {
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
            self.get_addons(service);
            self.previous_category = self.selected_category;
        }

        // TODO: add sorting and filtering similar to Installed view.
        // TODO: add table pagination instead of hard limiting search results.
        let mut addon_id = None;
        egui::CentralPanel::default().show_inside(ui, |ui| {
            if self.displayed_addons.is_polling() {
                ui.spinner();
                return;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut installed = false;
                egui::Grid::new("addon_grid")
                    .striped(true)
                    .spacing([5.0, 20.0])
                    .show(ui, |ui| {
                        // only show not-installed addons in search results
                        for addon in self.displayed_addons.value.as_ref().unwrap().iter() {
                            let selected = ui_show_addon_item(ui, addon);
                            if selected.is_some() {
                                addon_id = selected;
                            }
                            ui.horizontal_centered(|ui| {
                                if !addon.installed && ui.button("Install").clicked() {
                                    self.install_addon(addon.id, service);
                                    installed = true;
                                }
                            });
                            ui.end_row();
                        }
                    });
                if installed {
                    self.get_addons(service);
                }
            });
        });
        addon_id
    }
}
