use egui::Color32;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}
pub struct Colors {
    pub bg: Color32,
    pub panel: Color32,
    pub surface: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub border: Color32,
    pub danger: Color32,
    pub hovered: Color32,
    pub active: Color32,
    pub row: Color32,
}
impl Theme {
    pub fn colors(self) -> Colors {
        let rgb = Color32::from_rgb;
        match self {
            Self::Dark => Colors {
                bg: rgb(17, 19, 24),
                panel: rgb(24, 27, 34),
                surface: rgb(42, 49, 63),
                text: rgb(232, 235, 242),
                muted: rgb(158, 167, 183),
                accent: rgb(151, 174, 216),
                border: rgb(53, 59, 72),
                danger: rgb(255, 151, 151),
                hovered: rgb(47, 53, 66),
                active: rgb(54, 63, 81),
                row: rgb(21, 24, 30),
            },
            Self::Light => Colors {
                bg: rgb(246, 247, 250),
                panel: rgb(255, 255, 255),
                surface: rgb(226, 232, 242),
                text: rgb(27, 34, 46),
                muted: rgb(83, 94, 111),
                accent: rgb(52, 80, 127),
                border: rgb(193, 202, 215),
                danger: rgb(164, 41, 48),
                hovered: rgb(232, 237, 245),
                active: rgb(211, 222, 239),
                row: rgb(239, 242, 247),
            },
        }
    }
}
