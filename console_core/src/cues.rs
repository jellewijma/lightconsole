use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FixtureValues {
    pub intensity: Option<u8>,
    pub r: Option<u8>,
    pub g: Option<u8>,
    pub b: Option<u8>,
}

impl FixtureValues {
    /// Tracking: only overwrite fields that are Some() in `delta`.
    pub fn apply_delta(&mut self, delta: &FixtureValues) {
        if let Some(v) = delta.intensity {
            self.intensity = Some(v);
        }
        if let Some(v) = delta.r {
            self.r = Some(v);
        }
        if let Some(v) = delta.g {
            self.g = Some(v);
        }
        if let Some(v) = delta.b {
            self.b = Some(v);
        }
    }

    pub fn is_all_none(&self) -> bool {
        self.intensity.is_none() && self.r.is_none() && self.g.is_none() && self.b.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cue {
    pub number: u32,
    pub label: String,

    #[serde(default)]
    pub block: bool,

    #[serde(default)]
    pub fade_ms: u32,

    #[serde(default)]
    pub delay_ms: u32,

    /// Changes recorded in this cue (tracking style).
    pub changes: BTreeMap<u32, FixtureValues>, // fixture_id -> delta values
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CueList {
    pub cues: BTreeMap<u32, Cue>,
}

impl CueList {
    pub fn ensure() -> Self {
        Self::default()
    }
}
