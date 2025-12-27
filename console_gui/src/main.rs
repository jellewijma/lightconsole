use console_core::progcmd::{ApplyStatus, try_apply_programmer_line};
use console_core::{Runtime, Show};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const GRID_COLS: i32 = 8;
const GRID_ROWS: i32 = 5;

// Visual sizing
const CELL_PX: f32 = 84.0;
const GRID_LINE_ALPHA: u8 = 25;
const HANDLE_RADIUS: f32 = 7.0;

// Behavior
const MIN_W: i32 = 2;
const MIN_H: i32 = 1;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: cargo run -p console_gui -- <show.json>");
        std::process::exit(2);
    }

    let show_path = PathBuf::from(&args[1]);
    let layout_path = layout_path_for_show(&show_path);

    let app = GridApp::new(show_path, layout_path);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("LightConsole - Grid Zone")
            .with_inner_size([1280.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "LightConsole - Grid Zone",
        native_options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
}

fn layout_path_for_show(show_path: &Path) -> PathBuf {
    let stem = show_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("show");
    let file = format!("{stem}.layout.json");
    show_path.with_file_name(file)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum ContainerKind {
    Cues,
    Groups,
    Palettes,
}

impl ContainerKind {
    fn title(self) -> &'static str {
        match self {
            ContainerKind::Cues => "Cues",
            ContainerKind::Groups => "Groups",
            ContainerKind::Palettes => "Palettes",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum CellItem {
    Placeholder { label: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Container {
    id: u32,
    kind: ContainerKind,
    title: String,

    // grid coords
    x: i32,
    y: i32,
    w: i32,
    h: i32,

    #[serde(default)]
    cells: Vec<Option<CellItem>>,
}

impl Container {
    fn rect_cells(&self) -> (i32, i32, i32, i32) {
        (self.x, self.y, self.w, self.h)
    }

    fn idx(&self, cx: i32, cy: i32) -> usize {
        (cy * self.w + cx) as usize
    }

    fn get_cell(&self, cx: i32, cy: i32) -> Option<&CellItem> {
        self.cells.get(self.idx(cx, cy)).and_then(|v| v.as_ref())
    }

    fn set_cell(&mut self, cx: i32, cy: i32, item: Option<CellItem>) {
        let i = self.idx(cx, cy);
        if i < self.cells.len() {
            self.cells[i] = item;
        }
    }

    fn ensure_cells_len(&mut self) {
        let need = (self.w * self.h).max(0) as usize;
        if self.cells.len() != need {
            let mut new_cells = vec![None; need];
            // best-effort preserve sequentially
            for (i, v) in self.cells.iter().cloned().enumerate().take(need) {
                new_cells[i] = v;
            }
            self.cells = new_cells;
        }
    }

    fn resize_preserve(&mut self, new_w: i32, new_h: i32) {
        let old_w = self.w;
        let old_h = self.h;
        let old_cells = std::mem::take(&mut self.cells);

        self.w = new_w;
        self.h = new_h;

        let need = (self.w * self.h) as usize;
        self.cells = vec![None; need];

        let copy_w = old_w.min(self.w);
        let copy_h = old_h.min(self.h);

        // copy overlapping region
        for y in 0..copy_h {
            for x in 0..copy_w {
                let old_i = (y * old_w + x) as usize;
                let new_i = (y * self.w + x) as usize;
                if old_i < old_cells.len() && new_i < self.cells.len() {
                    self.cells[new_i] = old_cells[old_i].clone();
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Layout {
    cols: i32,
    rows: i32,
    next_id: u32,
    containers: Vec<Container>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            cols: GRID_COLS,
            rows: GRID_ROWS,
            next_id: 1,
            containers: vec![],
        }
    }
}

#[derive(Debug, Clone)]
enum DragState {
    None,
    Move {
        id: u32,
        grab_offset_px: egui::Vec2, // mouse - top-left(px)
    },
    Resize {
        id: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EncoderBank {
    Color,     // R G B
    Intensity, // I (single)
}

impl Default for EncoderBank {
    fn default() -> Self {
        EncoderBank::Color
    }
}

#[derive(Debug, Default)]
struct ProgrammerUi {
    // command console
    log: Vec<String>,
    line: String,

    // encoder bank
    bank: EncoderBank,
    r: u8,
    g: u8,
    b: u8,
    intensity: u8,
}

impl ProgrammerUi {
    fn push_digit(&mut self, d: char) {
        self.line.push(d);
    }

    fn push_dot(&mut self) {
        self.line.push('.');
    }

    fn push_token(&mut self, tok: &str) {
        if !self.line.is_empty() && !self.line.ends_with(' ') {
            self.line.push(' ');
        }
        self.line.push_str(tok);
        self.line.push(' ');
    }

    fn backspace(&mut self) {
        // trim trailing spaces first (feels nicer)
        while self.line.ends_with(' ') {
            self.line.pop();
        }
        self.line.pop();
    }

    fn clear_line(&mut self) {
        self.line.clear();
    }

    fn submit(&mut self) {
        let cmd = self.line.trim().to_string();
        if !cmd.is_empty() {
            self.log.push(format!("> {}", cmd));
        }
        self.line.clear();
    }
}

/// Minimal rotary knob (drag up/down to change).
fn knob_u8(ui: &mut egui::Ui, id: egui::Id, value: &mut u8, enabled: bool) {
    let size = egui::vec2(56.0, 56.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::drag());

    let mut v = *value as f32;

    if enabled && resp.dragged() {
        let dy = resp.drag_delta().y;
        // drag up = increase
        v += (-dy) * 0.6;
        v = v.clamp(0.0, 255.0);
        *value = v.round() as u8;
    }

    let painter = ui.painter();
    let center = rect.center();
    let r = 24.0;

    let bg = if enabled {
        egui::Color32::from_rgb(40, 40, 44)
    } else {
        egui::Color32::from_rgb(28, 28, 30)
    };
    painter.circle_filled(center, r, bg);
    painter.circle_stroke(
        center,
        r,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(90, 90, 95)),
    );

    // indicator line: map 0..255 to -135..+135 degrees
    let t = (*value as f32) / 255.0;
    let ang = (-135.0 + 270.0 * t).to_radians();
    let end = egui::pos2(
        center.x + ang.cos() * (r - 6.0),
        center.y + ang.sin() * (r - 6.0),
    );
    painter.line_segment(
        [center, end],
        egui::Stroke::new(2.0, egui::Color32::from_rgb(220, 220, 220)),
    );

    // tooltip
    if resp.hovered() {
        resp.on_hover_text(format!("{}", *value));
    }

    // keep id "used" (prevents warnings if you later expand)
    let _ = id;
}

struct GridApp {
    show_path: PathBuf,
    layout_path: PathBuf,
    layout: Layout,
    selected_id: Option<u32>,
    drag: DragState,
    dirty: bool,

    selected_cell: Option<(u32, i32, i32)>, // (container_id, cx, cy)

    next_cue: u32,
    next_group: u32,
    next_palette: u32,

    rt: Runtime,
    programmer_ui: ProgrammerUi,
    programmer_status: ApplyStatus, // optional but very helpful
}

impl GridApp {
    fn new(show_path: PathBuf, layout_path: PathBuf) -> Self {
        let mut layout = load_layout(&layout_path).unwrap_or_default();
        for c in &mut layout.containers {
            c.ensure_cells_len();
        }

        let show = Show::load_json_file(&show_path).unwrap_or_else(|e| {
            eprintln!("Failed to load show {:?}: {e}", show_path);
            Show::new("New Show")
        });

        let rt = Runtime::new(show);
        Self {
            show_path,
            layout_path,
            layout,
            selected_id: None,
            drag: DragState::None,
            dirty: false,
            selected_cell: None,
            next_cue: 1,
            next_group: 1,
            next_palette: 1,
            rt,
            programmer_status: ApplyStatus::NotProgrammer,
            programmer_ui: ProgrammerUi {
                bank: EncoderBank::Color,
                ..Default::default()
            },
        }
    }

    fn apply_programmer_preview(&mut self) {
        match try_apply_programmer_line(&self.programmer_ui.line, &mut self.rt.programmer) {
            ApplyStatus::Applied => {
                // Optional: show some UI indicator
            }
            ApplyStatus::Incomplete => {
                // Optional: show "..."
            }
            ApplyStatus::NotProgrammer => {
                // Do nothing: let other GUI shortcuts/commands handle it later
            }
        }
    }

    fn submit_programmer_line(&mut self) {
        let cmd = self.programmer_ui.line.trim().to_string();
        if cmd.is_empty() {
            return;
        }

        self.programmer_ui.log.push(format!("> {}", cmd));

        // Final apply (in case someone pasted text and pressed enter fast)
        let _ = try_apply_programmer_line(&cmd, &mut self.rt.programmer);

        self.programmer_ui.line.clear();
    }

    fn save_layout(&mut self) {
        if let Err(e) = save_layout(&self.layout_path, &self.layout) {
            eprintln!("Failed to save layout: {e}");
        } else {
            self.dirty = false;
        }
    }

    fn add_container_fill_row(&mut self, kind: ContainerKind) {
        // default size
        let w = self.layout.cols;
        let h = MIN_H;

        // find first y where a full-row container of height h fits without overlap
        let mut placed: Option<i32> = None;
        'outer: for y in 0..=(self.layout.rows - h) {
            let candidate = (0, y, w, h);
            for c in &self.layout.containers {
                if rects_intersect(candidate, c.rect_cells()) {
                    continue 'outer;
                }
            }
            placed = Some(y);
            break;
        }

        let Some(y) = placed else {
            // No space on this page
            return;
        };

        let id = self.layout.next_id;
        self.layout.next_id += 1;

        let title = kind.title().to_string();

        let mut c = Container {
            id,
            kind,
            title,
            x: 0,
            y,
            w,
            h,
            cells: vec![None; (w * h) as usize],
        };
        c.ensure_cells_len();
        self.layout.containers.push(c);

        self.selected_id = Some(id);
        self.dirty = true;
    }
}

impl eframe::App for GridApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Top bar
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Show: {}",
                    self.show_path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("?")
                ));
                ui.label("•");
                ui.label(format!(
                    "Layout: {}{}",
                    self.layout_path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("layout.json"),
                    if self.dirty { " (modified)" } else { "" }
                ));

                ui.separator();

                if ui.button("+ Cues row").clicked() {
                    self.add_container_fill_row(ContainerKind::Cues);
                }
                if ui.button("+ Groups row").clicked() {
                    self.add_container_fill_row(ContainerKind::Groups);
                }
                if ui.button("+ Palettes row").clicked() {
                    self.add_container_fill_row(ContainerKind::Palettes);
                }

                ui.separator();

                if ui.button("Save Layout").clicked() {
                    self.save_layout();
                }
            });
        });

        const PROGRAMMER_W: f32 = 560.0; // tweak to taste

        egui::SidePanel::right("programmer_panel")
            .resizable(false)
            .exact_width(PROGRAMMER_W)
            .show(ctx, |ui| {
                ui.heading("Programmer");

                // ----- Command console -----
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_height(140.0);
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in &self.programmer_ui.log {
                                ui.label(line);
                            }
                        });
                });

                // console output
                ui.separator();
                egui::CollapsingHeader::new("DMX Out (non-zero)")
                    .default_open(true)
                    .show(ui, |ui| match self.rt.render() {
                        Ok(live) => {
                            for (u, addr, val) in live.nonzero().into_iter().take(80) {
                                ui.monospace(format!("U{}:{:03} = {}", u, addr, val));
                            }
                        }
                        Err(e) => {
                            ui.label(format!("render error: {e:?}"));
                        }
                    });

                ui.label(format!("Selected: {:?}", self.rt.programmer.selected));
                ui.label(format!("Intensity: {:?}", self.rt.programmer.intensity));

                ui.label(match self.programmer_status {
                    ApplyStatus::Applied => "OK",
                    ApplyStatus::Incomplete => "...",
                    ApplyStatus::NotProgrammer => "",
                });

                ui.separator();

                // command entry line
                ui.horizontal(|ui| {
                    let resp: egui::Response =
                        ui.text_edit_singleline(&mut self.programmer_ui.line);

                    if resp.changed() {
                        self.apply_programmer_preview();
                    }

                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.submit_programmer_line();
                    }
                });

                ui.separator();

                // ----- Encoder bank -----
                ui.horizontal(|ui| {
                    ui.label("Encoders:");
                    if ui
                        .selectable_label(self.programmer_ui.bank == EncoderBank::Color, "Color")
                        .clicked()
                    {
                        self.programmer_ui.bank = EncoderBank::Color;
                    }
                    if ui
                        .selectable_label(
                            self.programmer_ui.bank == EncoderBank::Intensity,
                            "Intensity",
                        )
                        .clicked()
                    {
                        self.programmer_ui.bank = EncoderBank::Intensity;
                    }
                });

                ui.horizontal(|ui| {
                    let bank = self.programmer_ui.bank;

                    ui.vertical_centered(|ui| {
                        let v = match bank {
                            EncoderBank::Color => self.programmer_ui.r,
                            _ => self.programmer_ui.intensity,
                        };
                        ui.label(format!("{v}"));
                        knob_u8(
                            ui,
                            ui.id().with("knob1"),
                            match bank {
                                EncoderBank::Color => &mut self.programmer_ui.r,
                                EncoderBank::Intensity => &mut self.programmer_ui.intensity,
                            },
                            true,
                        );
                        ui.label(match bank {
                            EncoderBank::Color => "R",
                            EncoderBank::Intensity => "I",
                        });
                    });

                    ui.add_space(8.0);

                    ui.vertical_centered(|ui| {
                        let enabled = bank == EncoderBank::Color;
                        ui.label(format!("{}", self.programmer_ui.g));
                        knob_u8(
                            ui,
                            ui.id().with("knob2"),
                            &mut self.programmer_ui.g,
                            enabled,
                        );
                        ui.label("G");
                    });

                    ui.add_space(8.0);

                    ui.vertical_centered(|ui| {
                        let enabled = bank == EncoderBank::Color;
                        ui.label(format!("{}", self.programmer_ui.b));
                        knob_u8(
                            ui,
                            ui.id().with("knob3"),
                            &mut self.programmer_ui.b,
                            enabled,
                        );
                        ui.label("B");
                    });
                });

                ui.separator();

                // ----- Keypad + shortcuts -----
                ui.horizontal(|ui| {
                    // Keypad (4x5 with Enter spanning 2 columns)
                    let key = egui::vec2(66.0, 44.0);
                    let spacing_x = ui.spacing().item_spacing.x;
                    let enter_w = key.x * 2.0 + spacing_x;

                    ui.vertical(|ui| {
                        // Row 1: <- / - +
                        ui.horizontal(|ui| {
                            if ui.add_sized(key, egui::Button::new("←")).clicked() {
                                self.programmer_ui.backspace();
                                self.apply_programmer_preview();
                            }
                            if ui.add_sized(key, egui::Button::new("/")).clicked() {
                                self.programmer_ui.push_token("/");
                                self.apply_programmer_preview();
                            }
                            if ui.add_sized(key, egui::Button::new("-")).clicked() {
                                self.programmer_ui.push_token("-");
                                self.apply_programmer_preview();
                            }
                            if ui.add_sized(key, egui::Button::new("+")).clicked() {
                                self.programmer_ui.push_token("+");
                                self.apply_programmer_preview();
                            }
                        });
                        // Row 2: 7 8 9 thru
                        ui.horizontal(|ui| {
                            if ui.add_sized(key, egui::Button::new("7")).clicked() {
                                self.programmer_ui.push_digit('7');
                                self.apply_programmer_preview();
                            }
                            if ui.add_sized(key, egui::Button::new("8")).clicked() {
                                self.programmer_ui.push_digit('8');
                                self.apply_programmer_preview();
                            }
                            if ui.add_sized(key, egui::Button::new("9")).clicked() {
                                self.programmer_ui.push_digit('9');
                                self.apply_programmer_preview();
                            }
                            if ui.add_sized(key, egui::Button::new("thru")).clicked() {
                                self.programmer_ui.push_token("thru");
                                self.apply_programmer_preview();
                            }
                        });
                        // Row 3: 4 5 6 full
                        ui.horizontal(|ui| {
                            if ui.add_sized(key, egui::Button::new("4")).clicked() {
                                self.programmer_ui.push_digit('4');
                                self.apply_programmer_preview();
                            }
                            if ui.add_sized(key, egui::Button::new("5")).clicked() {
                                self.programmer_ui.push_digit('5');
                                self.apply_programmer_preview();
                            }
                            if ui.add_sized(key, egui::Button::new("6")).clicked() {
                                self.programmer_ui.push_digit('6');
                                self.apply_programmer_preview();
                            }
                            if ui.add_sized(key, egui::Button::new("full")).clicked() {
                                self.programmer_ui.push_token("full");
                                self.apply_programmer_preview();
                            }
                        });
                        // Row 4: 1 2 3 @
                        ui.horizontal(|ui| {
                            if ui.add_sized(key, egui::Button::new("1")).clicked() {
                                self.programmer_ui.push_digit('1');
                                self.apply_programmer_preview();
                            }
                            if ui.add_sized(key, egui::Button::new("2")).clicked() {
                                self.programmer_ui.push_digit('2');
                                self.apply_programmer_preview();
                            }
                            if ui.add_sized(key, egui::Button::new("3")).clicked() {
                                self.programmer_ui.push_digit('3');
                                self.apply_programmer_preview();
                            }
                            if ui.add_sized(key, egui::Button::new("@")).clicked() {
                                self.programmer_ui.push_token("@");
                                self.apply_programmer_preview();
                            }
                        });
                        // Row 5: 0 . Enter (Enter spans 2 columns)
                        ui.horizontal(|ui| {
                            if ui.add_sized(key, egui::Button::new("0")).clicked() {
                                self.programmer_ui.push_digit('0');
                                self.apply_programmer_preview();
                            }
                            if ui.add_sized(key, egui::Button::new(".")).clicked() {
                                self.programmer_ui.push_dot();
                                self.apply_programmer_preview();
                            }
                            if ui
                                .add_sized(egui::vec2(enter_w, key.y), egui::Button::new("Enter"))
                                .clicked()
                            {
                                self.programmer_ui.submit();
                                self.apply_programmer_preview();
                            }
                        });
                    });

                    ui.separator();

                    // Shortcuts column
                    ui.vertical(|ui| {
                        let b = egui::vec2(120.0, 40.0);

                        if ui.add_sized(b, egui::Button::new("Record")).clicked() {
                            self.programmer_ui.push_token("record");
                        }
                        if ui.add_sized(b, egui::Button::new("Update")).clicked() {
                            self.programmer_ui.push_token("update");
                        }
                        if ui.add_sized(b, egui::Button::new("Delete")).clicked() {
                            self.programmer_ui.push_token("delete");
                        }

                        ui.separator();

                        if ui.add_sized(b, egui::Button::new("Color")).clicked() {
                            self.programmer_ui.bank = EncoderBank::Color;
                            self.programmer_ui.push_token("color");
                        }
                        if ui.add_sized(b, egui::Button::new("Intensity")).clicked() {
                            self.programmer_ui.bank = EncoderBank::Intensity;
                            self.programmer_ui.push_token("intensity");
                        }

                        ui.separator();

                        if ui.add_sized(b, egui::Button::new("Clear Line")).clicked() {
                            self.programmer_ui.clear_line();
                        }
                        if ui.add_sized(b, egui::Button::new("Clear Log")).clicked() {
                            self.programmer_ui.log.clear();
                        }
                    });
                });
            });

        // Main canvas
        egui::CentralPanel::default().show(ctx, |ui| {
            // Scrollable canvas (so fixed CELL_PX works on smaller windows)
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let canvas_w = self.layout.cols as f32 * CELL_PX;
                    let canvas_h = self.layout.rows as f32 * CELL_PX;

                    let (resp, painter) = ui.allocate_painter(
                        egui::Vec2::new(canvas_w, canvas_h),
                        egui::Sense::click_and_drag(),
                    );

                    let origin = resp.rect.min;

                    // Background
                    painter.rect_filled(resp.rect, 0.0, egui::Color32::from_rgb(18, 18, 20));

                    // Grid lines
                    let grid_col =
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, GRID_LINE_ALPHA);
                    for x in 0..=self.layout.cols {
                        let px = origin.x + x as f32 * CELL_PX;
                        painter.line_segment(
                            [
                                egui::pos2(px, origin.y),
                                egui::pos2(px, origin.y + canvas_h),
                            ],
                            egui::Stroke::new(1.0, grid_col),
                        );
                    }
                    for y in 0..=self.layout.rows {
                        let py = origin.y + y as f32 * CELL_PX;
                        painter.line_segment(
                            [
                                egui::pos2(origin.x, py),
                                egui::pos2(origin.x + canvas_w, py),
                            ],
                            egui::Stroke::new(1.0, grid_col),
                        );
                    }

                    // --- Input handling ---
                    // We handle click/drag ourselves based on where the pointer hits (header or handle)
                    let pointer = ctx.input(|i| i.pointer.clone());
                    let pointer_pos = pointer.interact_pos();

                    if let Some(pos) = pointer_pos {
                        for c in self.layout.containers.iter().rev() {
                            let r = container_rect_px(origin, c);
                            if !r.contains(pos) {
                                continue;
                            }
                            let cx = ((pos.x - r.min.x) / CELL_PX).floor() as i32;
                            let cy = ((pos.y - r.min.y) / CELL_PX).floor() as i32;
                            if cx >= 0 && cy >= 0 && cx < c.w && cy < c.h {
                                break;
                            }
                        }
                    }

                    // Start interactions
                    let pointer_pos = ctx.input(|i| i.pointer.interact_pos());
                    let left_pressed =
                        ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
                    let right_pressed =
                        ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary));

                    let mut hit_handle: Option<u32> = None;
                    let mut hit_cell: Option<(u32, i32, i32)> = None;

                    if let Some(pos) = pointer_pos {
                        // handle hit
                        for c in self.layout.containers.iter().rev() {
                            let r = container_rect_px(origin, c);
                            let center = handle_center_px(r);
                            if pos.distance(center) <= HANDLE_RADIUS + 3.0 {
                                hit_handle = Some(c.id);
                                break;
                            }
                        }

                        // cell hit
                        for c in self.layout.containers.iter().rev() {
                            let r = container_rect_px(origin, c);
                            if !r.contains(pos) {
                                continue;
                            }
                            let cx = ((pos.x - r.min.x) / CELL_PX).floor() as i32;
                            let cy = ((pos.y - r.min.y) / CELL_PX).floor() as i32;
                            if cx >= 0 && cy >= 0 && cx < c.w && cy < c.h {
                                hit_cell = Some((c.id, cx, cy));
                                break;
                            }
                        }
                    }

                    if left_pressed {
                        if let Some(pos) = pointer_pos {
                            if let Some(id) = hit_handle {
                                self.selected_id = Some(id);
                                self.selected_cell = None;
                                self.drag = DragState::Resize { id };
                            } else if let Some((id, cx, cy)) = hit_cell {
                                self.selected_id = Some(id);
                                self.selected_cell = Some((id, cx, cy));

                                // header cell (0,0) -> move only
                                if cx == 0 && cy == 0 {
                                    let Some(c) =
                                        self.layout.containers.iter().find(|c| c.id == id)
                                    else {
                                        return;
                                    };
                                    let r = container_rect_px(origin, c);
                                    let grab_offset = pos - r.min;
                                    self.drag = DragState::Move {
                                        id,
                                        grab_offset_px: grab_offset,
                                    };
                                } else {
                                    // body cell -> place placeholder if empty
                                    self.drag = DragState::None;

                                    if let Some(idx) =
                                        self.layout.containers.iter().position(|c| c.id == id)
                                    {
                                        let c = &mut self.layout.containers[idx];
                                        c.ensure_cells_len();
                                        if c.get_cell(cx, cy).is_none() {
                                            let label = match c.kind {
                                                ContainerKind::Cues => {
                                                    let s = format!("Cue {}", self.next_cue);
                                                    self.next_cue += 1;
                                                    s
                                                }
                                                ContainerKind::Groups => {
                                                    let s = format!("Grp {}", self.next_group);
                                                    self.next_group += 1;
                                                    s
                                                }
                                                ContainerKind::Palettes => {
                                                    let s = format!("Pal {}", self.next_palette);
                                                    self.next_palette += 1;
                                                    s
                                                }
                                            };
                                            c.set_cell(
                                                cx,
                                                cy,
                                                Some(CellItem::Placeholder { label }),
                                            );
                                            self.dirty = true;
                                        }
                                    }
                                }
                            } else {
                                self.selected_id = None;
                                self.selected_cell = None;
                                self.drag = DragState::None;
                            }
                        }
                    }

                    if right_pressed {
                        if let Some((id, cx, cy)) = hit_cell {
                            if !(cx == 0 && cy == 0) {
                                if let Some(idx) =
                                    self.layout.containers.iter().position(|c| c.id == id)
                                {
                                    let c = &mut self.layout.containers[idx];
                                    c.ensure_cells_len();
                                    c.set_cell(cx, cy, None);
                                    self.selected_id = Some(id);
                                    self.selected_cell = Some((id, cx, cy));
                                    self.dirty = true;
                                }
                            }
                        }
                    }

                    // Continue drag
                    if pointer.primary_down() {
                        if let Some(pos) = pointer_pos {
                            match self.drag.clone() {
                                DragState::Move { id, grab_offset_px } => {
                                    let Some(idx) =
                                        self.layout.containers.iter().position(|c| c.id == id)
                                    else {
                                        /* container deleted? */
                                        return;
                                    };

                                    let mut c = self.layout.containers[idx].clone();

                                    // compute new top-left px, then snap to cell
                                    let tl = pos - grab_offset_px;
                                    let (mut new_x, mut new_y) = px_to_cell(origin, tl);

                                    // clamp to bounds
                                    new_x = new_x.clamp(0, self.layout.cols - c.w);
                                    new_y = new_y.clamp(0, self.layout.rows - c.h);

                                    // prevent overlap
                                    let candidate = (new_x, new_y, c.w, c.h);
                                    if !would_overlap(&self.layout.containers, id, candidate) {
                                        c.x = new_x;
                                        c.y = new_y;
                                        self.layout.containers[idx] = c;
                                        self.dirty = true;
                                    }
                                }
                                DragState::Resize { id } => {
                                    let Some(idx) =
                                        self.layout.containers.iter().position(|c| c.id == id)
                                    else {
                                        return;
                                    };

                                    let mut c = self.layout.containers[idx].clone();
                                    let r = container_rect_px(origin, &c);

                                    // mouse position relative to container top-left
                                    let rel = pos - r.min;

                                    // desired size in cells (ceil so you can grow immediately)
                                    let mut new_w = ((rel.x / CELL_PX).ceil() as i32).max(MIN_W);
                                    let mut new_h = ((rel.y / CELL_PX).ceil() as i32).max(MIN_H);

                                    // clamp to bounds
                                    new_w = new_w.clamp(MIN_W, self.layout.cols - c.x);
                                    new_h = new_h.clamp(MIN_H, self.layout.rows - c.y);

                                    // prevent overlap
                                    let candidate = (c.x, c.y, new_w, new_h);
                                    if !would_overlap(&self.layout.containers, id, candidate) {
                                        c.resize_preserve(new_w, new_h);
                                        self.layout.containers[idx] = c;
                                        self.dirty = true;
                                    }
                                }
                                DragState::None => {}
                            }
                        }
                    }

                    // End drag
                    if pointer.any_released() {
                        self.drag = DragState::None;
                    }

                    // --- Render containers ---
                    // draw in insertion order; selection gets higher-contrast border
                    for c in &self.layout.containers {
                        let sel_cell = self.selected_cell.and_then(|(id, cx, cy)| {
                            if id == c.id { Some((cx, cy)) } else { None }
                        });
                        draw_container(
                            &painter,
                            origin,
                            c,
                            self.selected_id == Some(c.id),
                            sel_cell,
                        );
                    }
                });
        });

        // Optional: autosave when closing later. For now, manual save is enough.
    }
}

fn load_layout(path: &Path) -> anyhow::Result<Layout> {
    let text = std::fs::read_to_string(path)?;
    let l = serde_json::from_str::<Layout>(&text)?;
    Ok(l)
}

fn save_layout(path: &Path, layout: &Layout) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(layout)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn container_rect_px(origin: egui::Pos2, c: &Container) -> egui::Rect {
    let min = egui::pos2(
        origin.x + c.x as f32 * CELL_PX,
        origin.y + c.y as f32 * CELL_PX,
    );
    let max = egui::pos2(
        origin.x + (c.x + c.w) as f32 * CELL_PX,
        origin.y + (c.y + c.h) as f32 * CELL_PX,
    );
    egui::Rect::from_min_max(min, max)
}

fn handle_center_px(container: egui::Rect) -> egui::Pos2 {
    egui::pos2(container.max.x - 10.0, container.max.y - 10.0)
}

fn px_to_cell(origin: egui::Pos2, px: egui::Pos2) -> (i32, i32) {
    let x = ((px.x - origin.x) / CELL_PX).round() as i32;
    let y = ((px.y - origin.y) / CELL_PX).round() as i32;
    (x, y)
}

fn rects_intersect(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> bool {
    let (ax, ay, aw, ah) = a;
    let (bx, by, bw, bh) = b;

    let a_right = ax + aw;
    let a_bottom = ay + ah;
    let b_right = bx + bw;
    let b_bottom = by + bh;

    ax < b_right && a_right > bx && ay < b_bottom && a_bottom > by
}

fn would_overlap(
    containers: &[Container],
    moving_id: u32,
    candidate: (i32, i32, i32, i32),
) -> bool {
    for c in containers {
        if c.id == moving_id {
            continue;
        }
        if rects_intersect(candidate, c.rect_cells()) {
            return true;
        }
    }
    false
}

fn draw_container(
    painter: &egui::Painter,
    origin: egui::Pos2,
    c: &Container,
    selected: bool,
    selected_cell: Option<(i32, i32)>,
) {
    let r = container_rect_px(origin, c);

    let line = egui::Stroke::new(
        1.0,
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 40),
    );

    for y in 0..c.h {
        for x in 0..c.w {
            let cell_min = egui::pos2(r.min.x + x as f32 * CELL_PX, r.min.y + y as f32 * CELL_PX);
            let cell = egui::Rect::from_min_size(cell_min, egui::Vec2::new(CELL_PX, CELL_PX));

            let is_header = x == 0 && y == 0;

            if is_header {
                painter.rect_filled(
                    cell,
                    egui::Rounding {
                        nw: 6.0,
                        ne: 0.0,
                        sw: 0.0,
                        se: 0.0,
                    },
                    egui::Color32::from_rgb(45, 46, 50),
                );
                painter.text(
                    egui::pos2(cell.min.x + 6.0, cell.center().y),
                    egui::Align2::LEFT_CENTER,
                    format!("{}", c.title),
                    egui::FontId::proportional(13.0),
                    egui::Color32::from_rgb(230, 230, 230),
                );
            } else {
                let filled = c.get_cell(x, y).is_some();
                let bg = if filled {
                    egui::Color32::from_rgb(55, 56, 60)
                } else {
                    egui::Color32::from_rgb(28, 29, 32)
                };
                painter.rect_filled(cell, 0.0, bg);

                if let Some(CellItem::Placeholder { label }) = c.get_cell(x, y) {
                    painter.text(
                        cell.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(12.0),
                        egui::Color32::from_rgb(235, 235, 235),
                    );
                }
            }

            painter.rect_stroke(cell, 0.0, line);

            if let Some((sx, sy)) = selected_cell {
                if sx == x && sy == y {
                    painter.rect_stroke(
                        cell.shrink(1.0),
                        0.0,
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(220, 190, 40)),
                    );
                }
            }
        }
    }

    // Border
    let border = if selected {
        egui::Stroke::new(2.0, egui::Color32::from_rgb(220, 190, 40))
    } else {
        egui::Stroke::new(1.0, egui::Color32::from_rgb(70, 70, 75))
    };
    painter.rect_stroke(r, 6.0, border);

    // Resize handle: only show when selected
    if selected {
        let center = handle_center_px(r);
        painter.circle_filled(
            center,
            HANDLE_RADIUS,
            egui::Color32::from_rgb(200, 200, 200),
        );
        painter.circle_stroke(
            center,
            HANDLE_RADIUS,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(40, 40, 40)),
        );
    }
}
