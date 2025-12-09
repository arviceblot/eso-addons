use super::{
    ui_helpers::{
        truncate_len,
        ui_show_star,
        AddonResponse,
        AddonResponseType,
        PromisedValue,
        // ui_show_bbtree,
    },
    ResetView, View,
};
// use bbcode_tagger::BBTree;
use eframe::egui::{self, vec2, Image, ImageButton, Layout, RichText, ScrollArea};
use eso_addons_core::service::{
    result::{AddonImageResult, AddonShowDetails},
    AddonService,
};
// use tracing::info;

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
    // parsed_description: PromisedValue<BBTree>,
    // parsed_changelog: PromisedValue<BBTree>,
    view: DetailView,
    // show_raw_text: bool,
    images: PromisedValue<Vec<AddonImageResult>>,
    selected_image: String,
}

impl Details {
    fn poll(&mut self, _: &mut AddonService) {
        // main details
        self.details.poll();
        if self.details.is_ready() {
            self.details.handle();

            // if let Some(details) = self.details.value.as_ref().unwrap() {
            // we have details, no setup parse for details and changelog if present
            // if let Some(description) = details.description.as_ref() {
            // info!("Parsing BBCode for addon description: {}", details.id);
            // self.parsed_description
            //     .set(service.parse_bbcode(description.to_string()));
            // }
            // if let Some(changelog) = details.change_log.as_ref() {
            // info!("Parsing BBCode for addon changelog: {}", details.id);
            // self.parsed_changelog
            //     .set(service.parse_bbcode(changelog.to_string()));
            // }
            // }
        }

        // images
        self.images.poll();

        // self.parsed_description.poll();
        // self.parsed_changelog.poll();
    }
    pub fn set_addon(&mut self, addon_id: i32, service: &mut AddonService) {
        self.addon_id = addon_id;
        // get addon details from service
        self.details.set(service.get_addon_details(addon_id));
        // get addon image URLs
        self.images.set(service.get_addon_images(addon_id));
        // if we get a new addon, reset view to description
        self.view = DetailView::default();
        self.selected_image = String::default();
    }
}
impl View for Details {
    fn ui(
        &mut self,
        ctx: &egui::Context,
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
        egui::TopBottomPanel::top("detail_top").show(ctx, |ui| {
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
                ui.label(format!("🔁 Version {}", addon.version));
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
                // ui.checkbox(&mut self.show_raw_text, "Show Unformatted Text");
            });
            ui.add_space(5.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ScrollArea::vertical().show(ui, |ui| match self.view {
                DetailView::Description => {
                    // if self.parsed_description.is_ready() && !self.show_raw_text {
                    //     ui_show_bbtree(ui, self.parsed_description.value.as_ref().unwrap());
                    // } else {
                    ui.label(addon.description.as_ref().unwrap_or(&"".to_string()));
                    // }
                }
                DetailView::ChangeLog => {
                    // if self.parsed_changelog.is_ready() && !self.show_raw_text {
                    //     ui_show_bbtree(ui, self.parsed_changelog.value.as_ref().unwrap());
                    // } else {
                    ui.label(addon.change_log.as_ref().unwrap_or(&"".to_string()));
                    // }
                }
                DetailView::Pictures => {
                    if self.selected_image == String::default() {
                        // set selected to first image
                        if let Some(img) = self.images.value.as_ref().unwrap().first() {
                            img.image.clone_into(&mut self.selected_image);
                        }
                    }
                    egui::SidePanel::left("image_left")
                        .default_width(100.0)
                        .show(ctx, |ui| {
                            ScrollArea::vertical().show(ui, |ui| {
                                for image in self.images.value.as_ref().unwrap() {
                                    if ui
                                        .add(ImageButton::new(
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
                    egui::CentralPanel::default().show(ctx, |ui| {
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
