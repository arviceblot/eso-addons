use egui::text::LayoutJob;
use egui::{CollapsingHeader, Color32, Frame, Image, RichText, Sense, Window};

use crate::ir::{Block, HAlign, HiddenKind, Inline, build_blocks};
use crate::style::Style;

const QUOTE_BAR: Color32 = Color32::from_rgb(120, 120, 120);

pub struct BBView {
    blocks: Vec<Block>,
}

#[derive(Default)]
pub struct BBState {
    open_image: Option<String>,
}

struct Ctx<'b> {
    state: &'b mut BBState,
    spoiler_index: u32,
}

impl BBView {
    pub fn parse(src: &str) -> Self {
        let doc = bbcode::parse(src);
        let blocks = build_blocks(&doc.children, &Style::default());
        Self { blocks }
    }

    pub fn show(&self, ui: &mut egui::Ui, state: &mut BBState, id_salt: impl std::hash::Hash) {
        let id_base = ui.id().with("bbcode-egui").with(id_salt);
        ui.push_id(id_base, |ui| {
            ui.visuals_mut().indent_has_left_vline = false;
            let mut ctx = Ctx {
                state,
                spoiler_index: 0,
            };
            render_blocks(ui, &self.blocks, &mut ctx);
        });
        show_image_window(ui.ctx(), state);
    }
}

fn render_blocks(ui: &mut egui::Ui, blocks: &[Block], ctx: &mut Ctx<'_>) {
    for block in blocks {
        render_block(ui, block, ctx);
    }
}

fn render_block(ui: &mut egui::Ui, block: &Block, ctx: &mut Ctx<'_>) {
    match block {
        Block::Para(inlines) => render_paragraph(ui, inlines, ctx),
        Block::List { ordered, items } => {
            ui.indent("bb-list", |ui| {
                for (i, item) in items.iter().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        let bullet = if *ordered {
                            format!("{}.", i + 1)
                        } else {
                            "•".to_string()
                        };
                        ui.label(RichText::new(bullet).strong());
                        ui.vertical(|ui| render_blocks(ui, item, ctx));
                    });
                }
            });
        }
        Block::Indent(children) => {
            ui.indent("bb-indent", |ui| render_blocks(ui, children, ctx));
        }
        Block::Code(text) => {
            Frame::group(ui.style())
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.label(RichText::new(text).monospace());
                });
        }
        Block::Hidden(kind, children) => {
            let idx = ctx.spoiler_index;
            ctx.spoiler_index += 1;
            let label = match kind {
                HiddenKind::Spoiler => "Spoiler",
                HiddenKind::Blur => "Hidden",
            };
            CollapsingHeader::new(label)
                .id_salt(("bb-spoiler", idx))
                .default_open(false)
                .show(ui, |ui| render_blocks(ui, children, ctx));
        }
        Block::Quote(children) => {
            Frame::default()
                .inner_margin(egui::Margin {
                    left: 8,
                    right: 4,
                    top: 2,
                    bottom: 2,
                })
                .stroke(egui::Stroke::new(2.0, QUOTE_BAR))
                .show(ui, |ui| {
                    ui.indent("bb-quote", |ui| render_blocks(ui, children, ctx));
                });
        }
        Block::Align(halign, children) => {
            match halign {
                HAlign::Left => {
                    ui.with_layout(
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| render_blocks(ui, children, ctx),
                    );
                }
                HAlign::Center => {
                    ui.vertical_centered(|ui| render_blocks(ui, children, ctx));
                }
                HAlign::Right => {
                    ui.with_layout(
                        egui::Layout::top_down(egui::Align::Max),
                        |ui| render_blocks(ui, children, ctx),
                    );
                }
            }
        }
    }
}

fn render_paragraph(ui: &mut egui::Ui, inlines: &[Inline], ctx: &mut Ctx<'_>) {
    let pane_width = ui.available_width();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut current = LayoutJob::default();
        let mut current_has_content = false;
        for inline in inlines {
            match inline {
                Inline::Text { text, style } => {
                    if text.is_empty() {
                        continue;
                    }
                    let tf = style.to_text_format(ui);
                    current.append(text, 0.0, tf);
                    current_has_content = true;
                }
                Inline::Link { label, url } => {
                    flush_job(ui, &mut current, &mut current_has_content);
                    let mut job = LayoutJob::default();
                    let link_color = ui.visuals().hyperlink_color;
                    for inl in label {
                        if let Inline::Text { text, style } = inl {
                            let mut s = style.clone();
                            s.color = Some(link_color);
                            s.underline = true;
                            let tf = s.to_text_format(ui);
                            job.append(text, 0.0, tf);
                        }
                    }
                    if job.is_empty() {
                        job.append(
                            url,
                            0.0,
                            egui::TextFormat {
                                color: link_color,
                                underline: egui::Stroke::new(1.0, link_color),
                                font_id: egui::FontId::proportional(14.0),
                                ..Default::default()
                            },
                        );
                    }
                    let resp = ui.add(egui::Label::new(job).sense(Sense::click()));
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    }
                }
                Inline::Image(url) => {
                    flush_job(ui, &mut current, &mut current_has_content);
                    // TODO(egui_extras image MIME bug): some servers send a
                    // parameterised Content-Type (e.g. `image/png; charset=UTF-8`).
                    // egui_extras 0.33 ImageCrateLoader::is_supported_mime does an
                    // exact string match against image::ImageFormat::from_mime_type,
                    // which only knows canonical names — so the bytes load fine but
                    // the loader returns FormatNotSupported and a red triangle is
                    // shown. Fixed upstream in egui_extras 0.34 by stripping the
                    // `;` parameter; revisit when we upgrade.
                    let resp = ui.add(
                        Image::new(url.as_str())
                            .fit_to_original_size(1.0)
                            .max_width(pane_width)
                            .maintain_aspect_ratio(true)
                            .sense(Sense::click()),
                    )
                    .on_hover_text(short_host(url));
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        ctx.state.open_image = Some(url.clone());
                    }
                }
                Inline::Youtube(id) => {
                    flush_job(ui, &mut current, &mut current_has_content);
                    let url = format!("https://www.youtube.com/watch?v={id}");
                    let label = format!("▶ youtube:{id}");
                    if ui.link(label).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    }
                }
            }
        }
        flush_job(ui, &mut current, &mut current_has_content);
    });
}

fn flush_job(ui: &mut egui::Ui, job: &mut LayoutJob, has_content: &mut bool) {
    if !*has_content {
        return;
    }
    let taken = std::mem::take(job);
    ui.label(taken);
    *has_content = false;
}

fn short_host(url: &str) -> String {
    let after_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = after_scheme.split(['/', '?', '#']).next().unwrap_or(url);
    host.to_string()
}

fn show_image_window(ctx: &egui::Context, state: &mut BBState) {
    let Some(url) = state.open_image.clone() else {
        return;
    };
    let mut open = true;
    Window::new("Image")
        .open(&mut open)
        .resizable(true)
        .default_size([640.0, 480.0])
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&url).small().monospace());
                    if ui.small_button("Open in browser").clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(url.clone()));
                    }
                });
                ui.separator();
                egui::ScrollArea::both().show(ui, |ui| {
                    ui.add(Image::new(url.as_str()).shrink_to_fit());
                });
            });
        });
    if !open {
        state.open_image = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn short_host_strips_path() {
        assert_eq!(short_host("https://i.imgur.com/foo.png"), "i.imgur.com");
        assert_eq!(short_host("http://example.com/a?b=1"), "example.com");
    }
}
