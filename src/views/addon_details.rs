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
    bb_description: Option<BBView>,
    bb_description_state: BBState,
    bb_changelog: Option<BBView>,
    bb_changelog_state: BBState,
    images: PromisedValue<Vec<AddonImageResult>>,
    selected_image: String,
}

impl Details {
    fn poll(&mut self, _: &mut AddonService) {
        self.details.poll();
        if self.details.is_ready() {
            self.details.handle();
            self.build_bb_views();
        }
        self.images.poll();
    }

    fn build_bb_views(&mut self) {
        if self.bb_description.is_some() && self.bb_changelog.is_some() {
            return;
        }
        let Some(Some(addon)) = self.details.value.as_ref() else {
            return;
        };
        let desc = addon.description.as_deref().unwrap_or("");
        let cl = addon.change_log.as_deref().unwrap_or("");
        self.bb_description = Some(BBView::parse(desc));
        self.bb_changelog = Some(BBView::parse(cl));
    }

    pub fn set_addon(&mut self, addon_id: i32, service: &mut AddonService) {
        self.addon_id = addon_id;
        self.details.set(service.get_addon_details(addon_id));
        self.images.set(service.get_addon_images(addon_id));
        self.view = DetailView::default();
        self.selected_image = String::default();
        self.bb_description = None;
        self.bb_changelog = None;
        self.bb_description_state = BBState::default();
        self.bb_changelog_state = BBState::default();
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

        let Some(Some(addon)) = self.details.value.as_ref() else {
            ui.label("No addon!");
            return response;
        };
        egui::TopBottomPanel::top("detail_top").show(ctx, |ui| {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                //close button
                if ui.button(RichText::new("⮪ Close").heading()).clicked() {
                    response.response_type = AddonResponseType::Close;
                }

                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if addon.is_upgradable()
                        && ui.button(RichText::new("⮉ Update").heading()).clicked()
                    {
                        response.addon_id = addon.id;
                        response.response_type = AddonResponseType::Update;
                    }
                    if !addon.installed {
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
                    response.author_name = addon.author_name.clone();
                    response.response_type = AddonResponseType::AuthorName;
                }
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.hyperlink_to("Visit Website", &addon.file_info_url);
                });
            });

            ui.horizontal(|ui| {
                // TODO: Add icon
                ui.label(addon.category.as_str());
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    // TODO: pretty format date like: January 1, 2024
                    ui.label(format!("🕘 Updated {}", addon.date));
                });
            });
            ui.horizontal(|ui| {
                if let (Some(name), Some(ver)) =
                    (&addon.game_compat_name, &addon.game_compat_version)
                {
                    ui.label(format!("⛭ {name} ({ver}) Supported"));
                } else {
                    ui.label("⛭ Unknown Version Supported");
                }
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    // TODO: pretty print download count
                    ui.label(format!(
                        "⮋ {} Downloads",
                        addon.download_total.as_deref().unwrap_or("")
                    ));
                });
            });
            ui.horizontal(|ui| {
                let mut version_text = addon.version.clone();
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
                        addon.favorite_total.as_deref().unwrap_or("")
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

        egui::CentralPanel::default().show(ctx, |ui| {
            ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| match self.view {
                    DetailView::Description => {
                        if self.show_raw_text {
                            ui.label(addon.description.as_deref().unwrap_or(""));
                        } else if let Some(view) = &self.bb_description {
                            view.show(ui, &mut self.bb_description_state, "description");
                        }
                    }
                    DetailView::ChangeLog => {
                        if self.show_raw_text {
                            ui.label(addon.change_log.as_deref().unwrap_or(""));
                        } else if let Some(view) = &self.bb_changelog {
                            view.show(ui, &mut self.bb_changelog_state, "change_log");
                        }
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
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.centered_and_justified(|ui| {
                                if self.selected_image != String::default() {
                                    ui.add(
                                        Image::new(self.selected_image.to_owned()).shrink_to_fit(),
                                    );
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
                                if let Some(download) = &addon.download {
                                    ui.label("Download Link");
                                    ui.hyperlink_to(truncate_len(download, 40), download);
                                    ui.end_row();
                                }
                                if let Some(md5) = &addon.md5 {
                                    ui.label("MD5");
                                    ui.code(md5.as_str());
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
        self.bb_description = None;
        self.bb_changelog = None;
    }
}
