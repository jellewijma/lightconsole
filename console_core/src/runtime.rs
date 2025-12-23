use crate::{FixtureValues, LiveState, Playback, Programmer, Show};
use std::collections::BTreeMap;

// Import the internal renderer from playback.rs
use crate::playback::render_fixture_values;

#[derive(Debug)]
pub struct Runtime {
    pub show: Show,
    pub playback_a: Playback,
    pub playback_b: Playback,
    pub programmer: Programmer,
}

impl Runtime {
    pub fn new(show: Show) -> Self {
        Self {
            show,
            playback_a: Playback::new("main"),
            playback_b: Playback::new("main"),
            programmer: Programmer::new(),
        }
    }

    pub fn tick(&mut self, dt_ms: u32) {
        self.playback_a.tick(dt_ms);
        self.playback_b.tick(dt_ms);
    }

    /// Render final DMX:
    /// 1) merge playback A + B at the *fixture-values* level (HTP/LTP)
    /// 2) render merged fixtures to LiveState
    /// 3) overlay programmer on top
    pub fn render(&self) -> anyhow::Result<LiveState> {
        let a = self.playback_a.output_state_map(&self.show)?;
        let b = self.playback_b.output_state_map(&self.show)?;

        let merged = merge_maps(&a, &b);

        let mut live = LiveState::new();
        for (fid, vals) in merged {
            // IMPORTANT: keep same argument order as your playback.rs signature
            render_fixture_values(&self.show, fid, &vals, &mut live)?;
        }

        let prog = self.programmer.render(&self.show)?;
        live.overlay(&prog);

        Ok(live)
    }
}

fn ltp(a: Option<u8>, b: Option<u8>) -> Option<u8> {
    match b {
        Some(_) => b, // playback B wins for LTP params
        None => a,
    }
}

fn htp(a: Option<u8>, b: Option<u8>) -> Option<u8> {
    if a.is_none() && b.is_none() {
        None
    } else {
        Some(a.unwrap_or(0).max(b.unwrap_or(0)))
    }
}

fn merge_fixture(a: Option<&FixtureValues>, b: Option<&FixtureValues>) -> FixtureValues {
    let a = a.cloned().unwrap_or_default();
    let b = b.cloned().unwrap_or_default();

    FixtureValues {
        // Intensity: HTP
        intensity: htp(a.intensity, b.intensity),
        // Color: LTP (B wins)
        r: ltp(a.r, b.r),
        g: ltp(a.g, b.g),
        b: ltp(a.b, b.b),
    }
}

fn merge_maps(
    a: &BTreeMap<u32, FixtureValues>,
    b: &BTreeMap<u32, FixtureValues>,
) -> BTreeMap<u32, FixtureValues> {
    let mut out = BTreeMap::new();

    for fid in a.keys().chain(b.keys()) {
        let fid = *fid;
        if out.contains_key(&fid) {
            continue;
        }
        out.insert(fid, merge_fixture(a.get(&fid), b.get(&fid)));
    }

    out
}
