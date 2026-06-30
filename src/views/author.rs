use eframe::egui::{self, RichText};
use eso_addons_core::service::AddonService;
use eso_addons_core::service::result::AddonShowDetails;

use super::ui_helpers::{AddonResponse, AddonResponseType, AddonTable, PromisedValue, Sort};
use super::{ResetView, View};

pub struct Author {
    author_name: String,
    addons: PromisedValue<Vec<AddonShowDetails>>,
    displayed_addons: Vec<AddonShowDetails>,
    sort: Sort,
    prev_sort: Sort,
    ascending: bool,
    prev_ascending: bool,
}
impl Default for Author {
    fn default() -> Self {
        Self {
            author_name: String::default(),
            addons: PromisedValue::default(),
            displayed_addons: vec![],
            sort: Sort::Name,
            prev_sort: Sort::Id,
            ascending: Sort::Name.default_ascending(),
            prev_ascending: Sort::Name.default_ascending(),
        }
    }
}
impl Author {
    pub fn author_name(&mut self, author_name: String, service: &AddonService) {
        self.author_name = author_name;
        self.get_addons(service);
    }
    fn poll(&mut self, service: &AddonService) {
        self.addons.poll_recording(service, "Loading author addons");
        if self.addons.is_ready() {
            self.addons.handle();
            self.sort_addons();
        }
    }
    fn handle_sort(&mut self) {
        if self.prev_sort != self.sort {
            self.ascending = self.sort.default_ascending();
        } else if self.prev_ascending == self.ascending {
            return;
        }
        self.prev_sort = self.sort;
        self.prev_ascending = self.ascending;
        self.sort_addons();
    }
    fn sort_addons(&mut self) {
        if let Some(addons) = self.addons.value.as_ref() {
            self.displayed_addons = addons.to_vec();
        }
        super::ui_helpers::sort_addons(&mut self.displayed_addons, self.sort, self.ascending);
    }
    fn get_addons(&mut self, service: &AddonService) {
        self.addons
            .set(service.get_addons_by_author(self.author_name.to_owned()));
    }
}
impl View for Author {
    fn ui(
        &mut self,
        _ctx: &eframe::egui::Context,
        ui: &mut eframe::egui::Ui,
        service: &mut AddonService,
    ) -> AddonResponse {
        let mut response = AddonResponse::default();
        self.poll(service);

        if self.addons.is_polling() {
            ui.spinner();
            return response;
        }
        self.handle_sort();

        egui::Panel::top("author_top").show_inside(ui, |ui| {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                //close button
                if ui.button(RichText::new("⮪ Close").heading()).clicked() {
                    response.response_type = AddonResponseType::Close;
                }
            });
            ui.add_space(5.0);

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(self.author_name.to_owned())
                        .heading()
                        .strong(),
                )
            });
            ui.add_space(5.0);
        });
        if response.response_type != AddonResponseType::default() {
            return response;
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            let show_addons: Vec<&AddonShowDetails> = self.displayed_addons.iter().collect();
            response = AddonTable::new(&show_addons).installable(true).ui(
                ui,
                &mut self.sort,
                &mut self.ascending,
            );
        });
        response
    }
}
impl ResetView for Author {
    fn reset(&mut self, service: &mut AddonService) {
        if self.author_name != String::default() {
            self.get_addons(service);
        }
    }
}
