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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Cue, CueList, FixtureInstance, FixtureValues, PlaybackMode, Show, default_fixture_types,
    };
    use std::collections::BTreeMap;

    fn make_test_show() -> anyhow::Result<Show> {
        let mut show = Show::new("Test Show");

        // fixture types (includes "rgb_par_3ch")
        for ft in default_fixture_types() {
            show.patch.add_fixture_type(ft);
        }

        // patch one RGB PAR at U1 @ 1 with fixture_id=1
        let f = FixtureInstance::new(1, "PAR 1", "rgb_par_3ch", 1, 1);
        show.patch.add_fixture(f)?;

        // ensure main cuelist exists
        show.cue_lists
            .entry("main".to_string())
            .or_insert_with(CueList::default);

        Ok(show)
    }

    #[test]
    fn programmer_overrides_playback() -> anyhow::Result<()> {
        let mut show = make_test_show()?;

        // cue 1 sets fixture 1 red = 200
        let mut changes = BTreeMap::new();
        changes.insert(
            1,
            FixtureValues {
                r: Some(200),
                ..Default::default()
            },
        );

        // IMPORTANT: if Cue has extra fields in your code, copy the Cue { ... } shape
        // from an existing test in console_core/src/playback.rs and keep values similar.
        let cue1 = Cue {
            number: 1,
            label: "PB Red".to_string(),
            changes,
            fade_ms: 0,
            delay_ms: 0,
            block: false,
        };

        show.cue_lists.get_mut("main").unwrap().cues.insert(1, cue1);

        // runtime
        let mut rt = Runtime::new(show);

        // playback A -> cue 1
        rt.playback_a.mode = PlaybackMode::CueOnly;
        rt.playback_a.goto(&rt.show, 1)?;

        // verify playback alone is red=200 at U1:001
        let pb_live = rt.playback_a.render(&rt.show)?;
        assert!(
            pb_live
                .nonzero()
                .iter()
                .any(|&(u, a, v)| u == 1 && a == 1 && v == 200),
            "expected playback to output U1:001=200"
        );

        // programmer overrides fixture 1 red = 10
        // ---- OPTION A (if you add a helper) ----
        // rt.programmer.set_fixture_values(1, FixtureValues { r: Some(10), ..Default::default() });

        // ---- OPTION B (no helper): use whatever your Programmer already uses ----
        // If your Programmer stores per-fixture values in a map called `values`:
        rt.programmer.selected.insert(1);
        rt.programmer.r = Some(10);

        let live = rt.render()?;
        assert!(
            live.nonzero()
                .iter()
                .any(|&(u, a, v)| u == 1 && a == 1 && v == 10),
            "expected runtime output U1:001=10 after programmer overlay"
        );
        assert!(
            !live
                .nonzero()
                .iter()
                .any(|&(u, a, v)| u == 1 && a == 1 && v == 200),
            "expected programmer to override playback (no U1:001=200)"
        );

        Ok(())
    }
}
