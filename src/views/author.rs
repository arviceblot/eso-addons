use eframe::egui::{self, RichText};
use eso_addons_core::service::result::AddonShowDetails;
use eso_addons_core::service::AddonService;

use super::ui_helpers::{AddonResponse, AddonResponseType, AddonTable, PromisedValue};
use super::{ResetView, View};

#[derive(Default)]
pub struct Author {
    author_name: String,
    addons: PromisedValue<Vec<AddonShowDetails>>,
}
impl Author {
    pub fn author_name(&mut self, author_name: String, service: &AddonService) {
        self.author_name = author_name;
        self.get_addons(service);
    }
    fn poll(&mut self) {
        self.addons.poll();
        if self.addons.is_ready() {
            self.addons.handle();
        }
    }
    fn get_addons(&mut self, service: &AddonService) {
        self.addons
            .set(service.get_addons_by_author(self.author_name.to_owned()));
    }
}
impl View for Author {
    fn ui(
        &mut self,
        ctx: &eframe::egui::Context,
        ui: &mut eframe::egui::Ui,
        _service: &mut AddonService,
    ) -> AddonResponse {
        let mut response = AddonResponse::default();
        self.poll();

        if self.addons.is_polling() {
            ui.spinner();
            return response;
        }

        egui::TopBottomPanel::top("author_top").show(ctx, |ui| {
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

        egui::CentralPanel::default().show(ctx, |ui| {
            let show_addons: Vec<&AddonShowDetails> =
                self.addons.value.as_ref().unwrap().iter().collect();
            response = AddonTable::new(&show_addons).installable(true).ui(ui);
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
