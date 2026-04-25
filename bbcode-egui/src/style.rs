use egui::{Color32, FontId, Stroke, TextFormat};

#[derive(Clone, Debug)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Option<Color32>,
    pub size_pt: f32,
    pub monospace: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            color: None,
            size_pt: 14.0,
            monospace: false,
        }
    }
}

impl Style {
    pub fn to_text_format(&self, ui: &egui::Ui) -> TextFormat {
        let visuals = ui.visuals();
        let base = visuals.text_color();
        let color = if self.bold {
            self.color.unwrap_or(visuals.strong_text_color())
        } else {
            self.color.unwrap_or(base)
        };
        let stroke_color = self.color.unwrap_or(base);
        let underline = if self.underline {
            Stroke::new(1.0, stroke_color)
        } else {
            Stroke::NONE
        };
        let strike = if self.strike {
            Stroke::new(1.0, stroke_color)
        } else {
            Stroke::NONE
        };
        let font_id = if self.monospace {
            FontId::monospace(self.size_pt)
        } else {
            FontId::proportional(self.size_pt)
        };
        let mut tf = TextFormat {
            font_id,
            color,
            underline,
            strikethrough: strike,
            italics: self.italic,
            ..Default::default()
        };
        if self.bold {
            tf.color = color;
        }
        tf
    }
}

pub fn size_from_attr(attr: Option<&str>, _base_pt: f32) -> Option<f32> {
    let raw = bbcode::unquote(attr?).trim();
    if raw.is_empty() {
        return None;
    }
    let (sign, num_str) = match raw.as_bytes()[0] {
        b'+' => (1i32, &raw[1..]),
        b'-' => (-1i32, &raw[1..]),
        _ => (0, raw),
    };
    let n: i32 = num_str.parse().ok()?;
    let level = if sign == 0 { n } else { 4 + sign * n };
    let level = level.clamp(1, 7);
    Some(SIZE_TABLE[(level - 1) as usize])
}

const SIZE_TABLE: [f32; 7] = [10.0, 12.0, 14.0, 16.0, 18.0, 22.0, 28.0];
