use crate::{LiveState, Playback, Programmer, Show};

#[derive(Debug)]
pub struct Runtime {
    pub show: Show,
    pub playback: Playback,
    pub programmer: Programmer,
}

impl Runtime {
    pub fn new(show: Show) -> Self {
        Self {
            show,
            playback: Playback::new("main"),
            programmer: Programmer::new(),
        }
    }

    pub fn tick(&mut self, dt_ms: u32) {
        self.playback.tick(dt_ms);
    }

    /// Render final DMX: playback first, then programmer overlay.
    pub fn render(&self) -> anyhow::Result<LiveState> {
        let mut pb = self.playback.render(&self.show)?;
        let prog = self.programmer.render(&self.show)?;
        pb.overlay(&prog); // modifies pb in-place
        Ok(pb) // return the modified state
    }
}
