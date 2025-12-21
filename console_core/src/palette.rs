use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaletteKind {
    Intensity,
    Color,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PaletteValues {
    pub intensity: Option<u8>,
    pub r: Option<u8>,
    pub g: Option<u8>,
    pub b: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Palette {
    pub kind: PaletteKind,
    pub values: PaletteValues,
}

impl Palette {
    pub fn new(kind: PaletteKind, values: PaletteValues) -> Self {
        Self { kind, values }
    }
}
