use eframe::egui::{self, Layout, RichText, ScrollArea};
use eso_addons_core::service::AddonService;

use super::View;
use super::ui_helpers::AddonResponse;

#[derive(Default)]
pub struct Errors {}

impl View for Errors {
    fn ui(
        &mut self,
        _ctx: &egui::Context,
        ui: &mut egui::Ui,
        service: &mut AddonService,
    ) -> AddonResponse {
        let response = AddonResponse::default();
        let errors = service.errors();

        ui.horizontal(|ui| {
            ui.heading(format!("Errors ({})", errors.len()));
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(!errors.is_empty(), egui::Button::new("Clear"))
                    .clicked()
                {
                    service.clear_errors();
                }
            });
        });
        ui.separator();

        if errors.is_empty() {
            ui.label("No errors recorded.");
            return response;
        }

        ScrollArea::vertical().show(ui, |ui| {
            for record in errors.iter().rev() {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(record.timestamp.format("%Y-%m-%d %H:%M:%S").to_string())
                                .monospace()
                                .weak(),
                        );
                        ui.label(RichText::new(&record.context).strong());
                    });
                    ui.label(&record.message);
                });
            }
        });

        response
    }
}
