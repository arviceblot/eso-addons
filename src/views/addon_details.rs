use super::{
    ResetView, View,
    ui_helpers::{AddonResponse, AddonResponseType, PromisedValue, truncate_len, ui_show_star},
};
use bbcode_egui::{BBState, BBView};
use eframe::egui::{self, Image, Layout, RichText, ScrollArea, vec2};
use egui::Button;
use eso_addons_core::service::{
    AddonService,
    result::{AddonImageResult, AddonShowDetails},
};

#[derive(PartialEq, Default)]
enum DetailView {
    #[default]
    Description,
    ChangeLog,
    Pictures,
    FileInfo,
}

#[derive(Default)]
pub struct Details {
    addon_id: i32,
    details: PromisedValue<Option<AddonShowDetails>>,
    view: DetailView,
    show_raw_text: bool,
    bb_description: BBState,
    bb_changelog: BBState,
    images: PromisedValue<Vec<AddonImageResult>>,
    selected_image: String,
}

impl Details {
    fn poll(&mut self, _: &mut AddonService) {
        self.details.poll();
        if self.details.is_ready() {
            self.details.handle();
        }
        self.images.poll();
    }
    pub fn set_addon(&mut self, addon_id: i32, service: &mut AddonService) {
        self.addon_id = addon_id;
        self.details.set(service.get_addon_details(addon_id));
        self.images.set(service.get_addon_images(addon_id));
        self.view = DetailView::default();
        self.selected_image = String::default();
        self.bb_description = BBState::default();
        self.bb_changelog = BBState::default();
    }
}
impl View for Details {
    fn ui(
        &mut self,
        _ctx: &egui::Context,
        ui: &mut egui::Ui,
        service: &mut AddonService,
    ) -> AddonResponse {
        let mut response = AddonResponse::default();
        self.poll(service);

        if self.details.is_polling() {
            ui.spinner();
            return response;
        }

        if self.details.value.as_ref().unwrap().is_none() {
            ui.label("No addon!");
            return response;
        }

        let addon = self
            .details
            .value
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .to_owned();
        egui::Panel::top("detail_top").show_inside(ui, |ui| {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                //close button
                if ui.button(RichText::new("⮪ Close").heading()).clicked() {
                    response.response_type = AddonResponseType::Close;
                }

                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if addon.to_owned().is_upgradable() {
                        // if self.is_updating_addon(addon.id) {
                        //     ui.add_enabled(false, egui::Button::new("Updating..."));
                        // } else if ui.button(RichText::new("⮉ Update").heading()).clicked() {
                        if ui.button(RichText::new("⮉ Update").heading()).clicked() {
                            response.addon_id = addon.id;
                            response.response_type = AddonResponseType::Update;
                        }
                    }
                    if !addon.installed {
                        // if self.is_installing_addon(addon.id) {
                        //     ui.add_enabled(false, egui::Button::new("Installing..."));
                        // } else if ui.button(RichText::new("⮋ Install").heading()).clicked() {
                        if ui.button(RichText::new("⮋ Install").heading()).clicked() {
                            response.addon_id = addon.id;
                            response.response_type = AddonResponseType::Install
                        }
                    } else if ui.button(RichText::new("🗙 Remove").heading()).clicked() {
                        response.response_type = AddonResponseType::Remove;
                        response.addon_id = addon.id;
                    }
                });
            });
            ui.add_space(5.0);

            ui.horizontal(|ui| {
                // ui.horizontal(|ui| {
                ui.label(RichText::new(addon.name.as_str()).heading().strong());
                if addon
                    .download_total
                    .as_ref()
                    .unwrap()
                    .parse::<i32>()
                    .unwrap()
                    > 5000
                {
                    ui_show_star(ui);
                }
                // });
            });
            ui.add_space(5.0);

            ui.horizontal(|ui| {
                if ui
                    .selectable_label(
                        false,
                        RichText::new(format!("by: {}", addon.author_name.as_str())),
                    )
                    .clicked()
                {
                    response.author_name = addon.to_owned().author_name;
                    response.response_type = AddonResponseType::AuthorName;
                }
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.hyperlink_to("Visit Website", addon.to_owned().file_info_url);
                });
            });

            ui.horizontal(|ui| {
                // TODO: Add icon
                ui.label(addon.to_owned().category);
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    // TODO: pretty format date like: January 1, 2024
                    ui.label(format!("🕘 Updated {}", addon.date));
                });
            });
            ui.horizontal(|ui| {
                if let Some(compat_version) = addon.to_owned().game_compat_version {
                    ui.label(format!(
                        "⛭ {} ({}) Supported",
                        addon.to_owned().game_compat_name.unwrap(),
                        compat_version
                    ));
                } else {
                    ui.label("⛭ Unknown Version Supported");
                }
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    // TODO: pretty print download count
                    ui.label(format!(
                        "⮋ {} Downloads",
                        addon.to_owned().download_total.unwrap_or("".to_string())
                    ));
                });
            });
            ui.horizontal(|ui| {
                let mut version_text = addon.version.to_string();
                if let Some(ref installed_version) = addon.installed_version
                    && *installed_version != addon.version
                {
                    version_text = format!("{} ➡ {}", installed_version, addon.version);
                }
                ui.label(format!("🔁 Version {}", version_text));
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    // TODO: pretty print favorite count
                    ui.label(format!(
                        "♥ {} Favorites",
                        addon.to_owned().favorite_total.unwrap_or("".to_string())
                    ));
                });
            });
            ui.separator();

            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.view,
                    DetailView::Description,
                    RichText::new("Details").heading(),
                );
                if self.images.is_ready() && !self.images.value.as_ref().unwrap().is_empty() {
                    ui.selectable_value(
                        &mut self.view,
                        DetailView::Pictures,
                        RichText::new("Images").heading(),
                    );
                }
                ui.selectable_value(
                    &mut self.view,
                    DetailView::FileInfo,
                    RichText::new("File Info").heading(),
                );
                ui.selectable_value(
                    &mut self.view,
                    DetailView::ChangeLog,
                    RichText::new("Change Log").heading(),
                );
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut self.show_raw_text, "Raw");
                });
            });
            ui.add_space(5.0);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| match self.view {
                DetailView::Description => {
                    let empty = String::new();
                    let text = addon.description.as_ref().unwrap_or(&empty);
                    if self.show_raw_text {
                        ui.label(text);
                    } else {
                        BBView::new(text).show(ui, &mut self.bb_description);
                    }
                }
                DetailView::ChangeLog => {
                    let empty = String::new();
                    let text = addon.change_log.as_ref().unwrap_or(&empty);
                    if self.show_raw_text {
                        ui.label(text);
                    } else {
                        BBView::new(text).show(ui, &mut self.bb_changelog);
                    }
                }
                DetailView::Pictures => {
                    if self.selected_image == String::default() {
                        // set selected to first image
                        if let Some(img) = self.images.value.as_ref().unwrap().first() {
                            img.image.clone_into(&mut self.selected_image);
                        }
                    }
                    egui::Panel::left("image_left")
                        .default_size(100.0)
                        .show_inside(ui, |ui| {
                            ScrollArea::vertical().show(ui, |ui| {
                                for image in self.images.value.as_ref().unwrap() {
                                    if ui
                                        .add(Button::image(
                                            Image::new(image.thumbnail.to_owned())
                                                .fit_to_exact_size(vec2(100.0, 100.0)),
                                        ))
                                        .clicked()
                                    {
                                        image.image.clone_into(&mut self.selected_image);
                                    }
                                }
                            });
                        });
                    egui::CentralPanel::default().show_inside(ui, |ui| {
                        ui.centered_and_justified(|ui| {
                            if self.selected_image != String::default() {
                                ui.add(Image::new(self.selected_image.to_owned()).shrink_to_fit());
                            }
                        });
                    });
                }
                DetailView::FileInfo => {
                    egui::Grid::new("my_grid")
                        .num_columns(2)
                        .spacing([40.0, 4.0])
                        .striped(true)
                        .show(ui, |ui| {
                            if let Some(download) = addon.download {
                                ui.label("Download Link");
                                ui.hyperlink_to(truncate_len(&download, 40), download);
                                ui.end_row();
                            }
                            if let Some(md5) = addon.md5 {
                                ui.label("MD5");
                                ui.code(md5);
                                ui.end_row();
                            }
                        });
                }
            });
        });
        response
    }
}
impl ResetView for Details {
    fn reset(&mut self, service: &mut AddonService) {
        // do not get if not addon id set yet
        if self.addon_id == i32::default() {
            return;
        }
        // re-get details for same addon
        self.details.set(service.get_addon_details(self.addon_id));
    }
}
