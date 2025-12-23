use anyhow::Context;
use console_core::{LiveState, Playback, PlaybackMode, Show};
use eframe::egui;
use std::time::Instant;

fn main() -> eframe::Result<()> {
    let show_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "show.json".to_string());

    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "LightConsole Snapshot",
        options,
        Box::new(|_cc| {
            Ok(Box::new(
                SnapshotApp::new(show_path).expect("failed to load show"),
            ))
        }),
    )
}

struct SnapshotApp {
    show_path: String,
    show: Show,
    playback: Playback,

    // UI state
    selected_cue: Option<u32>,
    run: bool,
    last_frame: Instant,
    last_error: Option<String>,
}

impl SnapshotApp {
    fn new(show_path: String) -> anyhow::Result<Self> {
        let show = Show::load_json_file(&show_path)
            .with_context(|| format!("load show file: {show_path}"))?;

        Ok(Self {
            show_path,
            show,
            playback: Playback::new("main"),
            selected_cue: None,
            run: false,
            last_frame: Instant::now(),
            last_error: None,
        })
    }

    fn cues(&self) -> Vec<u32> {
        self.show
            .cue_lists
            .get("main")
            .map(|cl| cl.cues.keys().copied().collect::<Vec<_>>())
            .unwrap_or_default()
    }

    fn safe_goto(&mut self, cue: u32) {
        if let Err(e) = self.playback.goto(&self.show, cue) {
            self.last_error = Some(format!("{e:#}"));
        }
    }

    fn safe_go(&mut self) {
        if let Err(e) = self.playback.go(&self.show) {
            self.last_error = Some(format!("{e:#}"));
        }
    }

    fn safe_tick(&mut self, ms: u32) {
        self.rt.tick(ms);
    }

    fn live(&mut self) -> LiveState {
        match self.playback.render(&self.show) {
            Ok(l) => l,
            Err(e) => {
                self.last_error = Some(format!("{e:#}"));
                LiveState::new()
            }
        }
    }

    fn transition_label(&self) -> String {
        match self.playback.transition_info() {
            Some((elapsed, delay, fade)) => {
                format!("Transition: elapsed={elapsed}ms  delay={delay}ms  fade={fade}ms")
            }
            None => "Transition: (none)".to_string(),
        }
    }
}

impl eframe::App for SnapshotApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Real-time ticking when Run is enabled
        if self.run {
            let now = Instant::now();
            let dt = now.duration_since(self.last_frame);
            self.last_frame = now;

            let ms = (dt.as_secs_f64() * 1000.0).round() as u32;
            if ms > 0 {
                self.safe_tick(ms.min(100)); // clamp so it stays stable
            }
            ctx.request_repaint(); // keep animating
        } else {
            self.last_frame = Instant::now();
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("LightConsole Snapshot");
                ui.separator();
                ui.label(format!("File: {}", self.show_path));
            });

            ui.horizontal(|ui| {
                // playback mode
                ui.label("Mode:");
                let mut mode = self.playback.mode;
                egui::ComboBox::from_id_source("mode")
                    .selected_text(format!("{mode:?}"))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut mode, PlaybackMode::Tracking, "Tracking");
                        ui.selectable_value(&mut mode, PlaybackMode::CueOnly, "CueOnly");
                    });
                self.playback.mode = mode;

                ui.separator();

                // cue picker
                let cue_nums = self.cues();
                if self.selected_cue.is_none() && !cue_nums.is_empty() {
                    self.selected_cue = Some(cue_nums[0]);
                }

                ui.label("Cue:");
                egui::ComboBox::from_id_source("cue_select")
                    .selected_text(
                        self.selected_cue
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "(none)".to_string()),
                    )
                    .show_ui(ui, |ui| {
                        for n in cue_nums {
                            ui.selectable_value(&mut self.selected_cue, Some(n), n.to_string());
                        }
                    });

                if ui.button("Goto").clicked() {
                    if let Some(c) = self.selected_cue {
                        self.safe_goto(c);
                    }
                }
                if ui.button("Go").clicked() {
                    self.safe_go();
                }

                ui.separator();

                // tick controls
                if ui.button("Tick 100ms").clicked() {
                    self.safe_tick(100);
                }
                if ui.button("Tick 500ms").clicked() {
                    self.safe_tick(500);
                }

                ui.separator();

                // run toggle
                if ui
                    .selectable_label(self.run, if self.run { "Running" } else { "Run" })
                    .clicked()
                {
                    self.run = !self.run;
                }
            });

            ui.label(format!(
                "Current cue: {:?} | {}",
                self.playback.current,
                self.transition_label()
            ));

            if let Some(err) = &self.last_error {
                ui.colored_label(egui::Color32::RED, format!("Error: {err}"));
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let live = self.live();
            let nonzero = live.nonzero();

            ui.heading("Non-zero DMX output");
            ui.separator();

            if nonzero.is_empty() {
                ui.label("(all zeros)");
                return;
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("dmx_grid")
                    .striped(true)
                    .num_columns(3)
                    .show(ui, |ui| {
                        ui.strong("Universe");
                        ui.strong("Address");
                        ui.strong("Value");
                        ui.end_row();

                        for (u, addr, v) in nonzero {
                            ui.label(format!("U{}", u));
                            ui.label(format!("{:03}", addr));
                            ui.label(format!("{}", v));
                            ui.end_row();
                        }
                    });
            });
        });
    }
}
