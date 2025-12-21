use anyhow::Context;
use std::collections::{BTreeMap, BTreeSet};

use crate::{ChannelKind, Show};
use crate::{Palette, PaletteKind, PaletteValues};

/// The Programmer is the live edit buffer:
/// - selection
/// - temporary values (intensity, rgb)
#[derive(Debug, Default, Clone)]
pub struct Programmer {
    pub selected: BTreeSet<u32>,
    pub intensity: Option<u8>, // 0..=255
    pub r: Option<u8>,
    pub g: Option<u8>,
    pub b: Option<u8>,
}

impl Programmer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear_all(&mut self) {
        self.selected.clear();
        self.intensity = None;
        self.r = None;
        self.g = None;
        self.b = None;
    }

    pub fn clear_values(&mut self) {
        self.intensity = None;
        self.r = None;
        self.g = None;
        self.b = None;
    }

    pub fn select_one(&mut self, id: u32) {
        self.selected.insert(id);
    }

    pub fn select_range(&mut self, a: u32, b: u32) {
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        for id in start..=end {
            self.selected.insert(id);
        }
    }

    pub fn set_intensity_percent(&mut self, pct: u8) {
        let pct = pct.min(100);
        // scale 0..100 -> 0..255
        let value = ((pct as u16 * 255) / 100) as u8;
        self.intensity = Some(value);
    }

    pub fn set_rgb(&mut self, r: u8, g: u8, b: u8) {
        self.r = Some(r);
        self.g = Some(g);
        self.b = Some(b);
    }

    /// Render ONLY the programmer into a fresh LiveState.
    /// (Later lessons will add playbacks, HTP/LTP merge, priorities, etc.)
    pub fn render(&self, show: &Show) -> anyhow::Result<LiveState> {
        let mut live = LiveState::new();

        for fixture_id in &self.selected {
            let f = show
                .patch
                .fixtures
                .get(fixture_id)
                .with_context(|| format!("unknown fixture id {}", fixture_id))?;

            let ft = show
                .patch
                .fixture_types
                .get(&f.fixture_type)
                .with_context(|| format!("unknown fixture type '{}'", f.fixture_type))?;

            for (i, ch) in ft.channels.iter().enumerate() {
                let addr = f.address + i as u16; // 1-based DMX
                if !(1..=512).contains(&addr) {
                    anyhow::bail!(
                        "fixture {} '{}' maps outside DMX range: U{} @ {} (channel index {})",
                        f.fixture_id,
                        f.name,
                        f.universe,
                        f.address,
                        i
                    );
                }

                let value_opt = match ch.kind {
                    ChannelKind::Intensity => self.intensity,
                    ChannelKind::ColorR => self.r,
                    ChannelKind::ColorG => self.g,
                    ChannelKind::ColorB => self.b,
                    _ => None,
                };

                if let Some(value) = value_opt {
                    live.set(f.universe, addr, value);
                }
            }
        }

        Ok(live)
    }

    pub fn snapshot_values(&self) -> PaletteValues {
        PaletteValues {
            intensity: self.intensity,
            r: self.r,
            g: self.g,
            b: self.b,
        }
    }

    pub fn apply_palette(&mut self, pal: &Palette) {
        match pal.kind {
            PaletteKind::Intensity => {
                if let Some(v) = pal.values.intensity {
                    self.intensity = Some(v);
                }
            }
            PaletteKind::Color => {
                if let Some(v) = pal.values.r {
                    self.r = Some(v);
                }
                if let Some(v) = pal.values.g {
                    self.g = Some(v);
                }
                if let Some(v) = pal.values.b {
                    self.b = Some(v);
                }
            }
        }
    }
}

/// Sparse DMX-like output:
/// Universe -> (Address -> Value)
#[derive(Debug, Clone, Default)]
pub struct LiveState {
    pub universes: BTreeMap<u16, BTreeMap<u16, u8>>,
}

impl LiveState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, universe: u16, address: u16, value: u8) {
        self.universes
            .entry(universe)
            .or_default()
            .insert(address, value);
    }

    /// Overlay: values in `top` overwrite values in `self`.
    pub fn overlay(&mut self, top: &LiveState) {
        for (&u, addrs) in &top.universes {
            let dst = self.universes.entry(u).or_default();
            for (&addr, &val) in addrs {
                dst.insert(addr, val);
            }
        }
    }

    pub fn nonzero(&self) -> Vec<(u16, u16, u8)> {
        let mut out = Vec::new();
        for (&u, addrs) in &self.universes {
            for (&addr, &v) in addrs {
                if v != 0 {
                    out.push((u, addr, v));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FixtureInstance, Show, default_fixture_types};

    #[test]
    fn render_rgb_par() -> anyhow::Result<()> {
        let mut show = Show::new("Test");
        for ft in default_fixture_types() {
            show.patch.add_fixture_type(ft);
        }

        show.patch
            .add_fixture(FixtureInstance::new(1, "PAR 1", "rgb_par_3ch", 1, 1))?;

        let mut p = Programmer::new();
        p.select_one(1);
        p.set_rgb(10, 20, 30);

        let live = p.render(&show)?;
        let nz = live.nonzero();

        assert!(nz.contains(&(1, 1, 10)));
        assert!(nz.contains(&(1, 2, 20)));
        assert!(nz.contains(&(1, 3, 30)));
        Ok(())
    }

    #[test]
    fn render_dimmer_intensity() -> anyhow::Result<()> {
        let mut show = Show::new("Test");
        for ft in default_fixture_types() {
            show.patch.add_fixture_type(ft);
        }

        show.patch
            .add_fixture(FixtureInstance::new(10, "DIM 1", "dimmer_1ch", 1, 100))?;

        let mut p = Programmer::new();
        p.select_one(10);
        p.set_intensity_percent(100);

        let live = p.render(&show)?;
        let nz = live.nonzero();

        assert!(nz.contains(&(1, 100, 255)));
        Ok(())
    }

    #[test]
    fn apply_color_palette_sets_rgb() {
        let mut p = Programmer::new();
        let pal = Palette::new(
            PaletteKind::Color,
            PaletteValues {
                r: Some(1),
                g: Some(2),
                b: Some(3),
                ..Default::default()
            },
        );

        p.apply_palette(&pal);

        assert_eq!(p.r, Some(1));
        assert_eq!(p.g, Some(2));
        assert_eq!(p.b, Some(3));
    }
}
