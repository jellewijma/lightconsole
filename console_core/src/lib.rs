use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub mod cues;
pub mod engine;
pub mod palette;
pub mod playback;
pub mod progcmd;

mod runtime;

pub use cues::{Cue, CueList, FixtureValues};
pub use engine::{LiveState, Programmer};
pub use palette::{Palette, PaletteKind, PaletteValues};
pub use playback::{Playback, PlaybackMode};
pub use runtime::Runtime;

pub fn version() -> &'static str {
    "0.1.0"
}

/// A show is the top-level document we save/load.
/// For now it only contains a Patch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Show {
    pub name: String,
    pub patch: Patch,

    #[serde(default)]
    pub palettes: BTreeMap<String, Palette>,

    #[serde(default)]
    pub cue_lists: BTreeMap<String, CueList>,

    #[serde(default)]
    pub groups: BTreeMap<String, BTreeSet<u32>>,
}

impl Show {
    pub fn new(name: impl Into<String>) -> Self {
        let mut cue_lists = BTreeMap::new();
        cue_lists.insert("main".into(), CueList::ensure());

        Self {
            name: name.into(),
            patch: Patch::default(),
            palettes: BTreeMap::new(),
            groups: BTreeMap::new(),
            cue_lists,
        }
    }

    /// Save the show to JSON.
    pub fn save_json_file(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self).context("serialize show to json")?;
        fs::write(path.as_ref(), json).context("write show json file")?;
        Ok(())
    }

    /// Load the show from JSON.
    pub fn load_json_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let text = fs::read_to_string(path.as_ref()).context("read show json file")?;
        let show = serde_json::from_str::<Show>(&text).context("parse show json")?;
        Ok(show)
    }
}

/// Patch holds fixture instances + fixture types.
/// In a pro console, the type library is huge; here we start tiny.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Patch {
    /// Fixture types known in this show (like a small fixture library).
    pub fixture_types: BTreeMap<String, FixtureType>,
    /// Actual patched fixtures.
    pub fixtures: BTreeMap<u32, FixtureInstance>,
}

impl Patch {
    pub fn add_fixture_type(&mut self, fixture_type: FixtureType) {
        self.fixture_types
            .insert(fixture_type.type_id.clone(), fixture_type);
    }

    pub fn add_fixture(&mut self, fixture: FixtureInstance) -> anyhow::Result<()> {
        if self.fixtures.contains_key(&fixture.fixture_id) {
            anyhow::bail!("Fixture ID {} already exists", fixture.fixture_id);
        }
        if !self.fixture_types.contains_key(&fixture.fixture_type) {
            anyhow::bail!(
                "Unknown fixture_type '{}'. Add the type first.",
                fixture.fixture_type
            );
        }
        self.fixtures.insert(fixture.fixture_id, fixture);
        Ok(())
    }

    pub fn list_fixtures(&self) -> Vec<&FixtureInstance> {
        self.fixtures.values().collect()
    }
}

/// Describes a fixture model/mode in a simplified way.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureType {
    pub type_id: String,
    pub manufacturer: String,
    pub model: String,
    pub channels: Vec<ChannelDef>,
}

/// One channel definition in a fixture type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelDef {
    pub name: String,
    pub kind: ChannelKind,
}

/// Very simplified categories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChannelKind {
    Intensity,
    Pan,
    Tilt,
    ColorR,
    ColorG,
    ColorB,
    Other,
}

/// A single fixture as patched into a universe/address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureInstance {
    pub fixture_id: u32,
    pub name: String,
    pub fixture_type: String,
    pub universe: u16,
    pub address: u16,
}

impl FixtureInstance {
    pub fn new(
        fixture_id: u32,
        name: impl Into<String>,
        fixture_type: impl Into<String>,
        universe: u16,
        address: u16,
    ) -> Self {
        Self {
            fixture_id,
            name: name.into(),
            fixture_type: fixture_type.into(),
            universe,
            address,
        }
    }
}

/// A helper: returns a default tiny “library” we can start with.
pub fn default_fixture_types() -> Vec<FixtureType> {
    vec![
        FixtureType {
            type_id: "rgb_par_3ch".to_string(),
            manufacturer: "Generic".to_string(),
            model: "RGB PAR (3ch)".to_string(),
            channels: vec![
                ChannelDef {
                    name: "Red".to_string(),
                    kind: ChannelKind::ColorR,
                },
                ChannelDef {
                    name: "Green".to_string(),
                    kind: ChannelKind::ColorG,
                },
                ChannelDef {
                    name: "Blue".to_string(),
                    kind: ChannelKind::ColorB,
                },
            ],
        },
        FixtureType {
            type_id: "dimmer_1ch".to_string(),
            manufacturer: "Generic".to_string(),
            model: "Dimmer (1ch)".to_string(),
            channels: vec![ChannelDef {
                name: "Intensity".to_string(),
                kind: ChannelKind::Intensity,
            }],
        },
    ]
}
