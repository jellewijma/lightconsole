use anyhow::Context;
use std::collections::BTreeMap;

use crate::{ChannelKind, FixtureValues, LiveState, Show};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackMode {
    Tracking,
    CueOnly,
}

#[derive(Debug, Clone)]
struct Transition {
    from: BTreeMap<u32, FixtureValues>, // fully-resolved: Some(...) for all fields
    to: BTreeMap<u32, FixtureValues>,   // fully-resolved
    elapsed_ms: u32,
    fade_ms: u32,
    delay_ms: u32,
}

#[derive(Debug, Clone)]
pub struct Playback {
    pub cuelist: String,
    pub current: Option<u32>,
    pub mode: PlaybackMode,
    transition: Option<Transition>,
}

impl Playback {
    pub fn new(cuelist: impl Into<String>) -> Self {
        Self {
            cuelist: cuelist.into(),
            current: None,
            mode: PlaybackMode::Tracking,
            transition: None,
        }
    }

    fn state_map_at(
        &self,
        show: &Show,
        cue_num: u32,
    ) -> anyhow::Result<BTreeMap<u32, FixtureValues>> {
        match self.mode {
            PlaybackMode::Tracking => self.tracked_state_at(show, cue_num),
            PlaybackMode::CueOnly => self.cue_only_state_at(show, cue_num),
        }
    }

    fn tracked_state_at(
        &self,
        show: &Show,
        cue_num: u32,
    ) -> anyhow::Result<BTreeMap<u32, FixtureValues>> {
        let list = show
            .cue_lists
            .get(&self.cuelist)
            .with_context(|| format!("unknown cuelist '{}'", self.cuelist))?;

        let mut tracked: BTreeMap<u32, FixtureValues> = BTreeMap::new();

        for (&num, cue) in &list.cues {
            if num > cue_num {
                break;
            }

            if cue.block {
                for &fid in cue.changes.keys() {
                    tracked.insert(fid, FixtureValues::default());
                }
            }

            for (&fid, delta) in &cue.changes {
                tracked.entry(fid).or_default().apply_delta(delta);
            }
        }

        Ok(tracked)
    }

    fn cue_only_state_at(
        &self,
        show: &Show,
        cue_num: u32,
    ) -> anyhow::Result<BTreeMap<u32, FixtureValues>> {
        let list = show
            .cue_lists
            .get(&self.cuelist)
            .with_context(|| format!("unknown cuelist '{}'", self.cuelist))?;

        Ok(list
            .cues
            .get(&cue_num)
            .map(|c| c.changes.clone())
            .unwrap_or_default())
    }

    fn resolve_map(mut m: BTreeMap<u32, FixtureValues>) -> BTreeMap<u32, FixtureValues> {
        for v in m.values_mut() {
            if v.intensity.is_none() {
                v.intensity = Some(0);
            }
            if v.r.is_none() {
                v.r = Some(0);
            }
            if v.g.is_none() {
                v.g = Some(0);
            }
            if v.b.is_none() {
                v.b = Some(0);
            }
        }
        m
    }

    pub fn output_state_map(&self, show: &Show) -> anyhow::Result<BTreeMap<u32, FixtureValues>> {
        if let Some(tr) = &self.transition {
            // During delay: hold the start look
            if tr.elapsed_ms < tr.delay_ms {
                return Ok(tr.from.clone());
            }

            // After delay: fade from -> to
            if tr.fade_ms == 0 {
                return Ok(tr.to.clone());
            }

            let t = (tr.elapsed_ms - tr.delay_ms).min(tr.fade_ms);
            return Ok(interpolate_maps(&tr.from, &tr.to, t, tr.fade_ms));
        }

        let Some(cur) = self.current else {
            return Ok(BTreeMap::new());
        };

        let raw = self.state_map_at(show, cur)?;
        Ok(Self::resolve_map(raw))
    }

    pub fn state_map(&self, show: &Show) -> anyhow::Result<BTreeMap<u32, FixtureValues>> {
        match self.mode {
            PlaybackMode::Tracking => self.tracked_state(show),
            PlaybackMode::CueOnly => self.cue_only_state(show),
        }
    }

    /// Returns the tracked fixture-values map up to the current cue.
    pub fn tracked_state(&self, show: &Show) -> anyhow::Result<BTreeMap<u32, FixtureValues>> {
        let Some(cur) = self.current else {
            return Ok(BTreeMap::new());
        };

        let list = show
            .cue_lists
            .get(&self.cuelist)
            .with_context(|| format!("unknown cuelist '{}'", self.cuelist))?;

        let mut tracked: BTreeMap<u32, FixtureValues> = BTreeMap::new();

        for (&num, cue) in &list.cues {
            if num > cur {
                break;
            }

            // BLOCKING: reset fixtures touched by this cue so nothing tracks through
            if cue.block {
                for &fid in cue.changes.keys() {
                    tracked.insert(fid, FixtureValues::default());
                }
            }

            for (&fid, delta) in &cue.changes {
                tracked.entry(fid).or_default().apply_delta(delta);
            }
        }

        Ok(tracked)
    }

    pub fn goto(&mut self, show: &Show, cue: u32) -> anyhow::Result<()> {
        self.activate(show, cue)
    }

    pub fn go(&mut self, show: &Show) -> anyhow::Result<Option<u32>> {
        let list = show
            .cue_lists
            .get(&self.cuelist)
            .with_context(|| format!("unknown cuelist '{}'", self.cuelist))?;

        let nums: Vec<u32> = list.cues.keys().copied().collect();
        if nums.is_empty() {
            self.current = None;
            self.transition = None;
            return Ok(None);
        }

        let next = match self.current {
            None => nums[0],
            Some(cur) => nums.into_iter().find(|n| *n > cur).unwrap_or(cur),
        };

        self.activate(show, next)?;
        Ok(self.current)
    }

    fn activate(&mut self, show: &Show, target: u32) -> anyhow::Result<()> {
        // IMPORTANT: capture the CURRENT visible output, even if we're mid-fade
        let from = self.output_state_map(show)?;

        // determine timing from target cue (if present)
        let (fade_ms, delay_ms) = {
            let list = show
                .cue_lists
                .get(&self.cuelist)
                .with_context(|| format!("unknown cuelist '{}'", self.cuelist))?;
            if let Some(cue) = list.cues.get(&target) {
                (cue.fade_ms, cue.delay_ms)
            } else {
                (0, 0)
            }
        };

        let to_raw = self.state_map_at(show, target)?;
        let to = Self::resolve_map(to_raw);

        self.current = Some(target);

        if fade_ms == 0 && delay_ms == 0 {
            self.transition = None;
            return Ok(());
        }

        self.transition = Some(Transition {
            from,
            to,
            elapsed_ms: 0,
            fade_ms,
            delay_ms,
        });

        Ok(())
    }

    pub fn tick(&mut self, dt_ms: u32) {
        if let Some(tr) = &mut self.transition {
            tr.elapsed_ms = tr.elapsed_ms.saturating_add(dt_ms);
            let done_at = tr.delay_ms.saturating_add(tr.fade_ms);
            if tr.elapsed_ms >= done_at {
                self.transition = None; // transition complete
            }
        }
    }

    fn cue_only_state(&self, show: &Show) -> anyhow::Result<BTreeMap<u32, FixtureValues>> {
        let Some(cur) = self.current else {
            return Ok(BTreeMap::new());
        };

        let list = show
            .cue_lists
            .get(&self.cuelist)
            .with_context(|| format!("unknown cuelist '{}'", self.cuelist))?;

        let cue = match list.cues.get(&cur) {
            Some(c) => c,
            None => return Ok(BTreeMap::new()),
        };

        // CueOnly: only this cue’s own changes
        Ok(cue.changes.clone())
    }

    pub fn transition_info(&self) -> Option<(u32, u32, u32)> {
        self.transition
            .as_ref()
            .map(|t| (t.elapsed_ms, t.delay_ms, t.fade_ms))
    }

    /// Render the tracked output of the cuelist at the current cue.
    pub fn render(&self, show: &Show) -> anyhow::Result<LiveState> {
        let state = self.output_state_map(show)?;
        let mut live = LiveState::new();

        for (fid, vals) in state {
            render_fixture_values(show, fid, &vals, &mut live)?;
        }

        Ok(live)
    }
}

fn lerp_u8(a: u8, b: u8, t: u32, dur: u32) -> u8 {
    if dur == 0 {
        return b;
    }
    let a = a as i32;
    let b = b as i32;
    let delta = b - a;
    let v = a + (delta * t as i32) / dur as i32;
    v.clamp(0, 255) as u8
}

fn interpolate_maps(
    from: &BTreeMap<u32, crate::FixtureValues>,
    to: &BTreeMap<u32, crate::FixtureValues>,
    t: u32,
    dur: u32,
) -> BTreeMap<u32, crate::FixtureValues> {
    let mut out = BTreeMap::new();

    let keys = from
        .keys()
        .chain(to.keys())
        .copied()
        .collect::<std::collections::BTreeSet<_>>();

    for fid in keys {
        let fa = from.get(&fid);
        let ta = to.get(&fid);

        let f = fa.unwrap_or(&crate::FixtureValues {
            intensity: Some(0),
            r: Some(0),
            g: Some(0),
            b: Some(0),
        });
        let tt = ta.unwrap_or(&crate::FixtureValues {
            intensity: Some(0),
            r: Some(0),
            g: Some(0),
            b: Some(0),
        });

        let fv = crate::FixtureValues {
            intensity: Some(lerp_u8(
                f.intensity.unwrap_or(0),
                tt.intensity.unwrap_or(0),
                t,
                dur,
            )),
            r: Some(lerp_u8(f.r.unwrap_or(0), tt.r.unwrap_or(0), t, dur)),
            g: Some(lerp_u8(f.g.unwrap_or(0), tt.g.unwrap_or(0), t, dur)),
            b: Some(lerp_u8(f.b.unwrap_or(0), tt.b.unwrap_or(0), t, dur)),
        };

        out.insert(fid, fv);
    }

    out
}

fn render_fixture_values(
    show: &Show,
    fixture_id: u32,
    vals: &FixtureValues,
    live: &mut LiveState,
) -> anyhow::Result<()> {
    let f = show
        .patch
        .fixtures
        .get(&fixture_id)
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
            ChannelKind::Intensity => vals.intensity,
            ChannelKind::ColorR => vals.r,
            ChannelKind::ColorG => vals.g,
            ChannelKind::ColorB => vals.b,
            _ => None,
        };

        if let Some(v) = value_opt {
            live.set(f.universe, addr, v);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cue, CueList, FixtureInstance, FixtureValues, Show, default_fixture_types};

    #[test]
    fn fade_interpolates_over_time() -> anyhow::Result<()> {
        use crate::{Cue, CueList, FixtureInstance, FixtureValues, Show, default_fixture_types};

        let mut show = Show::new("Test");
        for ft in default_fixture_types() {
            show.patch.add_fixture_type(ft);
        }
        show.patch
            .add_fixture(FixtureInstance::new(1, "PAR 1", "rgb_par_3ch", 1, 1))?;

        let mut cl = CueList::default();

        // Cue 1: explicit red 0 (baseline)
        cl.cues.insert(
            1,
            Cue {
                number: 1,
                label: "Base".into(),
                block: false,
                fade_ms: 0,
                delay_ms: 0,
                changes: [(
                    1u32,
                    FixtureValues {
                        r: Some(0),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            },
        );

        // Cue 2: red 255 with fade 1000ms
        cl.cues.insert(
            2,
            Cue {
                number: 2,
                label: "Fade to Red".into(),
                block: false,
                fade_ms: 1000,
                delay_ms: 0,
                changes: [(
                    1u32,
                    FixtureValues {
                        r: Some(255),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            },
        );

        show.cue_lists.insert("main".into(), cl);

        let mut pb = Playback::new("main");
        pb.goto(&show, 1)?;
        pb.goto(&show, 2)?; // start fade

        // at t=0, should still be from-state
        let st0 = pb.output_state_map(&show)?;
        assert_eq!(st0.get(&1).unwrap().r, Some(0));

        pb.tick(500);
        let st1 = pb.output_state_map(&show)?;
        assert_eq!(st1.get(&1).unwrap().r, Some(127)); // 255 * 500 / 1000 = 127

        pb.tick(500);
        let st2 = pb.output_state_map(&show)?;
        assert_eq!(st2.get(&1).unwrap().r, Some(255));

        Ok(())
    }
    #[test]
    fn tracking_works_across_cues() -> anyhow::Result<()> {
        let mut show = Show::new("Test");
        for ft in default_fixture_types() {
            show.patch.add_fixture_type(ft);
        }
        show.patch
            .add_fixture(FixtureInstance::new(1, "PAR 1", "rgb_par_3ch", 1, 1))?;

        // Cue 1: set Red only
        let mut cl = CueList::default();
        cl.cues.insert(
            1,
            Cue {
                number: 1,
                label: "Red".into(),
                fade_ms: 0,
                delay_ms: 0,
                block: false,
                changes: [(
                    1u32,
                    FixtureValues {
                        r: Some(255),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            },
        );

        // Cue 2: set Blue only -> Red should track from cue 1
        cl.cues.insert(
            2,
            Cue {
                number: 2,
                label: "Blue add".into(),
                fade_ms: 0,
                delay_ms: 0,
                block: false,
                changes: [(
                    1u32,
                    FixtureValues {
                        b: Some(255),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            },
        );

        show.cue_lists.insert("main".into(), cl);

        let mut pb = Playback::new("main");
        pb.goto(&show, 2)?;

        let live = pb.render(&show)?;
        let nz = live.nonzero();

        // PAR 1 @ U1:1..3 => R at 1, B at 3
        assert!(nz.contains(&(1, 1, 255)));
        assert!(nz.contains(&(1, 3, 255)));
        Ok(())
    }
}
