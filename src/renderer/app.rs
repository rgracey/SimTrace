//! Main application

use crate::coach::{CoachHandle, CoachStatus, CoachTip};
use crate::config::{AppSettings, ParsedColors, ReferenceLapStrategy};
use crate::core::{DataCollector, TelemetryBuffer};
use eframe::egui;
use egui::color_picker::{color_edit_button_srgba, Alpha};
use std::collections::VecDeque;
use std::sync::Arc;

// The buffer is kept larger than the maximum display window so the slider can
// show the full range without data disappearing at the top.
const BUFFER_CAPACITY_SECS: u64 = 60;
const MAX_DISPLAY_WINDOW_SECS: f32 = 30.0;
/// Polling rate for the background telemetry thread.
const POLL_RATE_HZ: u64 = 60;
/// Minimum overlay dimensions — keeps the layout from collapsing.
const MIN_WIDTH: f32 = 300.0;
const MIN_HEIGHT: f32 = 130.0;
/// Track position strip height and gap below the main content.
const STRIP_H: f32 = 10.0;
const STRIP_GAP: f32 = 3.0;

// ── Background poller ─────────────────────────────────────────────────────────

enum PollerCmd {
    ActivatePlugin(String),
}

/// Owns the background polling thread. Dropping this stops the thread.
struct PollerHandle {
    cmd_tx: std::sync::mpsc::Sender<PollerCmd>,
    _thread: std::thread::JoinHandle<()>,
}

// ── Palette ──────────────────────────────────────────────────────────────────
const BAR_BG: egui::Color32 = egui::Color32::from_rgb(13, 13, 13);
const CARD_BG: egui::Color32 = egui::Color32::from_rgb(16, 16, 16);
const BORDER: egui::Color32 = egui::Color32::from_rgb(26, 26, 26);
const LABEL_DIM: egui::Color32 = egui::Color32::from_rgb(90, 90, 90);
const LABEL_MID: egui::Color32 = egui::Color32::from_rgb(140, 140, 140);
const ACCENT_RED: egui::Color32 = egui::Color32::from_rgb(220, 45, 45);

pub struct SimTraceApp {
    settings: AppSettings,
    /// Shared buffer written by the background poller, read by the UI.
    buffer: Arc<TelemetryBuffer>,
    /// Background polling thread; `None` when stopped.
    poller: Option<PollerHandle>,
    current_steering: f32,
    /// Half-lock angle in degrees from the active plugin's config (e.g. 450 for 900° wheel).
    max_steering_angle: f32,
    running: bool,
    config_open: bool,
    minimized: bool,
    /// 0.0 = fully hidden, 1.0 = fully visible
    bar_alpha: f32,
    /// Tracks which plugin is currently active so we can detect dropdown changes
    active_plugin: String,
    /// Set when the user saves; drives a brief "Saved" toast
    save_toast: Option<std::time::Instant>,
    /// Colors pre-parsed from `settings.colors`; re-derived when config changes them.
    parsed_colors: ParsedColors,
    /// Lap boundary detection and per-lap telemetry for comparison.
    lap_store: crate::core::LapStore,
    /// Background coach thread; `None` when disabled or not yet started.
    coach: Option<CoachHandle>,
    /// Snapshots of relevant config at the time the coach was last spawned.
    /// Any change to these triggers a coach restart so the new settings take effect.
    coach_spawned_tts: bool,
    /// Last N tips received from the coach thread, newest last.
    coach_tips: VecDeque<CoachTip>,
    /// Latest status snapshot from the coach thread.
    coach_status: CoachStatus,
}

impl SimTraceApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals {
            panel_fill: egui::Color32::TRANSPARENT,
            window_fill: egui::Color32::TRANSPARENT,
            ..egui::Visuals::dark()
        });

        let settings = crate::config::AppSettings::load_or_default();
        let active_plugin = settings.collector.plugin.clone();
        let parsed_colors = ParsedColors::from_scheme(&settings.colors);
        let max_steering_angle = crate::plugins::create_plugin(&active_plugin)
            .map(|p| p.get_config().max_steering_angle)
            .unwrap_or(450.0);
        Self {
            settings,
            buffer: Arc::new(TelemetryBuffer::new(std::time::Duration::from_secs(
                BUFFER_CAPACITY_SECS,
            ))),
            poller: None,
            current_steering: 0.0,
            max_steering_angle,
            running: true,
            config_open: false,
            minimized: false,
            bar_alpha: 1.0,
            active_plugin,
            save_toast: None,
            parsed_colors,
            lap_store: crate::core::LapStore::new(),
            coach: None,
            coach_spawned_tts: false,
            coach_tips: VecDeque::new(),
            coach_status: CoachStatus::default(),
        }
    }

    fn start(&mut self) {
        let mut collector = DataCollector::new(BUFFER_CAPACITY_SECS);
        // Share the collector's buffer with the UI before moving the collector.
        self.buffer = collector.buffer();

        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<PollerCmd>();
        let plugin_name = self.settings.collector.plugin.clone();
        let poll_interval = std::time::Duration::from_micros(1_000_000 / POLL_RATE_HZ);

        let thread = std::thread::spawn(move || {
            let _ = collector.activate_plugin(&plugin_name);
            loop {
                collector.poll();
                loop {
                    match cmd_rx.try_recv() {
                        Ok(PollerCmd::ActivatePlugin(name)) => {
                            collector.buffer().clear();
                            let _ = collector.activate_plugin(&name);
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                    }
                }
                std::thread::sleep(poll_interval);
            }
        });

        self.poller = Some(PollerHandle {
            cmd_tx,
            _thread: thread,
        });
        self.running = true;
    }

    fn activate_plugin(&mut self) {
        let plugin = self.settings.collector.plugin.clone();
        if let Some(h) = &self.poller {
            let _ = h.cmd_tx.send(PollerCmd::ActivatePlugin(plugin.clone()));
        }
        self.buffer.clear();
        self.current_steering = 0.0;
        self.max_steering_angle = crate::plugins::create_plugin(&plugin)
            .map(|p| p.get_config().max_steering_angle)
            .unwrap_or(450.0);
        self.active_plugin = plugin;
        self.lap_store.clear();
    }
}

impl eframe::App for SimTraceApp {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = self.settings.save_to_config_path();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Force dark visuals every frame — prevents the OS light theme from
        // overriding our settings (observed on Windows 11 with light mode).
        ctx.set_visuals(egui::Visuals {
            panel_fill: egui::Color32::TRANSPARENT,
            window_fill: egui::Color32::TRANSPARENT,
            ..egui::Visuals::dark()
        });

        // Track window geometry for persistence (skip height when minimized)
        if let Some(inner) = ctx.input(|i| i.viewport().inner_rect) {
            self.settings.overlay.width = inner.width();
            if !self.minimized {
                self.settings.overlay.height = inner.height();
            }
        }
        if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
            self.settings.overlay.position_x = outer.min.x;
            self.settings.overlay.position_y = outer.min.y;
        }
        // ── Poller lifecycle ─────────────────────────────────────────────────
        if !self.running && self.poller.is_some() {
            self.poller = None; // drop sender → background thread exits
            self.buffer.clear();
            self.current_steering = 0.0;
        }
        if self.running && self.poller.is_none() {
            self.start();
        }
        if self.settings.collector.plugin != self.active_plugin {
            self.activate_plugin();
        }

        // ── Coach lifecycle ──────────────────────────────────────────────────
        let coach_should_run = self.settings.coach.enabled && self.running;
        // Restart if TTS toggled while running — it's built at thread start.
        let needs_restart = self.settings.coach.tts_enabled != self.coach_spawned_tts;
        if coach_should_run && self.coach.is_some() && needs_restart {
            self.coach = None; // drop → thread exits cleanly
        }
        if coach_should_run && self.coach.is_none() {
            self.coach_spawned_tts = self.settings.coach.tts_enabled;
            self.coach = Some(CoachHandle::spawn(
                self.settings.coach.clone(),
                self.buffer.clone(),
            ));
        }
        if !coach_should_run && self.coach.is_some() {
            self.coach = None; // drop sender → thread exits cleanly
        }

        // Drain tips and status updates from the coach thread.
        if let Some(ref handle) = self.coach {
            let (new_tips, new_status) = handle.drain();
            for tip in new_tips {
                self.coach_tips.push_back(tip);
                while self.coach_tips.len() > 3 {
                    self.coach_tips.pop_front();
                }
            }
            if let Some(s) = new_status {
                self.coach_status = s;
            }
        }

        // ── Read latest telemetry ────────────────────────────────────────────
        if self.running {
            if let Some(pt) = self.buffer.latest() {
                self.current_steering = pt.telemetry.steering_angle;
                self.lap_store.push(&pt);
            }
        }
        // Clone the Arc so the closure below can take &mut self freely.
        let buffer = if self.running {
            Some(self.buffer.clone())
        } else {
            None
        };
        // Clone the reference lap so the closure can read it without borrowing self.
        let reference_lap = self.lap_store.reference_lap.clone();

        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
            MIN_WIDTH, MIN_HEIGHT,
        )));

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let screen = ui.max_rect();
                let opacity = self.settings.overlay.opacity;
                let a = (opacity * 255.0) as u8;
                let bar_h = 26.0_f32;
                let pad = 2.0_f32;

                // ── Hover detection + bar fade ───────────────────────────────
                let hovered = ctx.input(|i| {
                    i.pointer
                        .hover_pos()
                        .map(|p| screen.contains(p))
                        .unwrap_or(false)
                });
                let target = if self.minimized || hovered || self.config_open {
                    1.0_f32
                } else {
                    0.0_f32
                };
                // Fast in, slow out
                let speed = if target > self.bar_alpha { 0.18 } else { 0.06 };
                self.bar_alpha += (target - self.bar_alpha) * speed;
                if (self.bar_alpha - target).abs() > 0.005 {
                    ctx.request_repaint(); // keep animating
                }
                let ba = (a as f32 * self.bar_alpha) as u8;

                // ── Title bar — same width as content card ───────────────────
                let bar_rect = egui::Rect::from_min_max(
                    egui::pos2(screen.min.x + pad, screen.min.y),
                    egui::pos2(screen.max.x - pad, screen.min.y + bar_h),
                );
                ui.painter().rect_filled(
                    bar_rect,
                    egui::CornerRadius {
                        nw: 5,
                        ne: 5,
                        sw: 0,
                        se: 0,
                    },
                    with_alpha(BAR_BG, ba),
                );

                // Red accent stripe along the top edge of the bar
                ui.painter().line_segment(
                    [bar_rect.min, egui::pos2(bar_rect.max.x, bar_rect.min.y)],
                    egui::Stroke::new(2.0, with_alpha(ACCENT_RED, ba)),
                );
                // Bottom divider
                ui.painter().line_segment(
                    [
                        egui::pos2(bar_rect.min.x, bar_rect.max.y),
                        egui::pos2(bar_rect.max.x, bar_rect.max.y),
                    ],
                    egui::Stroke::new(1.0, with_alpha(BORDER, ba)),
                );

                // Drag zone (always interactive, even when faded)
                let drag_resp = ui.allocate_rect(bar_rect, egui::Sense::click_and_drag());
                if drag_resp.dragged() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                // Brand label — left side
                ui.painter().text(
                    egui::pos2(bar_rect.min.x + 10.0, bar_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    "SIMTRACE",
                    egui::FontId::monospace(10.0),
                    with_alpha(LABEL_MID, ba),
                );

                // Grip lines — center of bar
                let gx = bar_rect.center().x;
                let gy = bar_rect.center().y;
                for dy in [-3.5_f32, 0.0, 3.5] {
                    ui.painter().line_segment(
                        [
                            egui::pos2(gx - 12.0, gy + dy),
                            egui::pos2(gx + 12.0, gy + dy),
                        ],
                        egui::Stroke::new(1.5, with_alpha(LABEL_DIM, ba)),
                    );
                }

                // Close (✕) button — far right
                let close_rect = egui::Rect::from_center_size(
                    egui::pos2(bar_rect.max.x - 14.0, bar_rect.center().y),
                    egui::vec2(22.0, 22.0),
                );
                let close_resp = ui.allocate_rect(close_rect, egui::Sense::click());
                if close_resp.hovered() {
                    ui.painter().rect_filled(
                        close_rect,
                        4.0,
                        egui::Color32::from_rgba_unmultiplied(80, 30, 30, ba),
                    );
                }
                ui.painter().text(
                    close_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "×",
                    egui::FontId::proportional(14.0),
                    with_alpha(LABEL_MID, ba),
                );
                if close_resp.clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }

                // Minimize button — between close and gear
                let minimize_rect = egui::Rect::from_center_size(
                    egui::pos2(bar_rect.max.x - 40.0, bar_rect.center().y),
                    egui::vec2(22.0, 22.0),
                );
                let minimize_resp = ui.allocate_rect(minimize_rect, egui::Sense::click());
                if self.minimized || minimize_resp.hovered() {
                    ui.painter().rect_filled(
                        minimize_rect,
                        4.0,
                        egui::Color32::from_rgba_unmultiplied(60, 60, 60, ba),
                    );
                }
                // Painted chevron: ˄ when minimized (expand), ˅ when visible (collapse)
                {
                    let cx = minimize_rect.center().x;
                    let cy = minimize_rect.center().y;
                    let w = 5.0_f32;
                    let h = 3.5_f32;
                    let stroke = egui::Stroke::new(1.5, with_alpha(LABEL_MID, ba));
                    if self.minimized {
                        ui.painter().line_segment(
                            [
                                egui::pos2(cx - w, cy + h * 0.5),
                                egui::pos2(cx, cy - h * 0.5),
                            ],
                            stroke,
                        );
                        ui.painter().line_segment(
                            [
                                egui::pos2(cx, cy - h * 0.5),
                                egui::pos2(cx + w, cy + h * 0.5),
                            ],
                            stroke,
                        );
                    } else {
                        ui.painter().line_segment(
                            [
                                egui::pos2(cx - w, cy - h * 0.5),
                                egui::pos2(cx, cy + h * 0.5),
                            ],
                            stroke,
                        );
                        ui.painter().line_segment(
                            [
                                egui::pos2(cx, cy + h * 0.5),
                                egui::pos2(cx + w, cy - h * 0.5),
                            ],
                            stroke,
                        );
                    }
                }
                if minimize_resp.clicked() {
                    self.minimized = !self.minimized;
                    if self.minimized {
                        self.config_open = false;
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                            self.settings.overlay.width,
                            bar_h + 4.0,
                        )));
                    } else {
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                            self.settings.overlay.width,
                            self.settings.overlay.height,
                        )));
                    }
                }

                // Gear button — left of minimize button
                let gear_rect = egui::Rect::from_center_size(
                    egui::pos2(bar_rect.max.x - 66.0, bar_rect.center().y),
                    egui::vec2(22.0, 22.0),
                );
                let gear_resp = ui.allocate_rect(gear_rect, egui::Sense::click());
                if self.config_open || gear_resp.hovered() {
                    ui.painter().rect_filled(
                        gear_rect,
                        4.0,
                        egui::Color32::from_rgba_unmultiplied(60, 60, 60, ba),
                    );
                }
                ui.painter().text(
                    gear_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "⚙",
                    egui::FontId::proportional(13.0),
                    with_alpha(
                        if self.config_open {
                            egui::Color32::WHITE
                        } else {
                            LABEL_MID
                        },
                        ba,
                    ),
                );
                if gear_resp.clicked() {
                    self.config_open = !self.config_open;
                }

                if !self.minimized {
                    // ── Content card — stadium shape (rounded right cap) ─────────
                    let content_rect = egui::Rect::from_min_max(
                        egui::pos2(screen.min.x + pad, screen.min.y + bar_h),
                        egui::pos2(screen.max.x - pad, screen.max.y - pad),
                    );
                    // Cap radius: half the card height → perfect semicircle on the right
                    let cap_r = content_rect.height() / 2.0;
                    ui.painter().add(egui::Shape::convex_polygon(
                        stadium_path(content_rect, 5.0, cap_r),
                        with_alpha(CARD_BG, a),
                        egui::Stroke::new(1.0, with_alpha(BORDER, a)),
                    ));

                    // Guard against the transition frame where the window hasn't
                    // resized yet (content_rect would have near-zero height).
                    if content_rect.height() > 10.0 {
                        if self.running {
                            // Reserve coach tip strip at the bottom if we have tips.
                            let coach_tip = self.coach_tips.back();
                            let tip_h = if coach_tip.is_some() { 20.0_f32 } else { 0.0 };
                            let telem_rect = egui::Rect::from_min_max(
                                content_rect.shrink(2.0).min,
                                egui::pos2(
                                    content_rect.shrink(2.0).max.x,
                                    content_rect.max.y - tip_h - 2.0,
                                ),
                            );

                            let mut content_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(telem_rect)
                                    .layout(egui::Layout::top_down(egui::Align::LEFT)),
                            );
                            draw_telemetry(
                                &mut content_ui,
                                &mut self.settings,
                                &self.parsed_colors,
                                buffer.as_ref(),
                                self.current_steering,
                                self.max_steering_angle,
                                a,
                                cap_r,
                                reference_lap.as_deref(),
                            );

                            // ── Coach tip strip ──────────────────────────────
                            if let Some(tip) = coach_tip {
                                let tip_rect = egui::Rect::from_min_max(
                                    egui::pos2(
                                        content_rect.min.x + 6.0,
                                        content_rect.max.y - tip_h,
                                    ),
                                    egui::pos2(
                                        content_rect.max.x - cap_r - 4.0,
                                        content_rect.max.y,
                                    ),
                                );
                                let age_secs = tip.generated_at.elapsed().as_secs_f32();
                                let tip_alpha =
                                    ((1.0 - (age_secs / 20.0).min(1.0)) * a as f32) as u8;
                                ui.painter().text(
                                    tip_rect.left_center(),
                                    egui::Align2::LEFT_CENTER,
                                    &tip.text,
                                    egui::FontId::proportional(9.5),
                                    egui::Color32::from_rgba_unmultiplied(200, 200, 255, tip_alpha),
                                );
                            }
                        } else {
                            let font_size = (content_rect.height() * 0.28).clamp(14.0, 42.0);
                            ui.painter().text(
                                content_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "STOPPED",
                                egui::FontId::monospace(font_size),
                                with_alpha(egui::Color32::from_gray(55), a),
                            );
                        }
                    } // end content_rect.height() > 10.0

                    // ── Resize handle — bottom-right corner of the rectangle ──────
                    {
                        // Right edge of the circle = content_rect.max.x, bottom = content_rect.max.y
                        let hx = content_rect.max.x;
                        let hy = content_rect.max.y;
                        let grip = 20.0_f32;
                        let hr = egui::Rect::from_min_max(
                            egui::pos2(hx - grip, hy - grip),
                            egui::pos2(hx, hy),
                        );
                        let resp = ui.allocate_rect(hr, egui::Sense::drag());
                        if resp.dragged() {
                            let delta = resp.drag_delta();
                            if let Some(inner) = ctx.input(|i| i.viewport().inner_rect) {
                                let new_size = egui::vec2(
                                    (inner.width() + delta.x).max(MIN_WIDTH),
                                    (inner.height() + delta.y).max(MIN_HEIGHT),
                                );
                                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(new_size));
                            }
                        }
                        // Diagonal grip lines in the corner
                        let p = ui.painter();
                        for i in 1..=3i32 {
                            let o = i as f32 * 5.0;
                            p.line_segment(
                                [egui::pos2(hx - o, hy), egui::pos2(hx, hy - o)],
                                egui::Stroke::new(1.5, with_alpha(LABEL_DIM, ba)),
                            );
                        }
                    }
                } // end !self.minimized

                // ── Config panel ─────────────────────────────────────────────
                if self.config_open {
                    let panel_w = 260.0_f32.min(screen.width() - 8.0);
                    let panel_top = screen.min.y + bar_h + 4.0;
                    let panel_rect = egui::Rect::from_min_size(
                        egui::pos2(screen.max.x - panel_w - 4.0, panel_top),
                        egui::vec2(panel_w, (screen.max.y - panel_top - 4.0).max(40.0)),
                    );
                    ui.painter()
                        .rect_filled(panel_rect, 6.0, egui::Color32::from_rgb(20, 20, 20));
                    ui.painter().rect_stroke(
                        panel_rect,
                        6.0,
                        egui::Stroke::new(1.0, BORDER),
                        egui::StrokeKind::Middle,
                    );
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(panel_rect.shrink(12.0))
                            .layout(egui::Layout::top_down(egui::Align::LEFT)),
                    );
                    egui::ScrollArea::vertical().show(&mut child, |ui| {
                        draw_config(
                            ui,
                            &mut self.settings,
                            &mut self.running,
                            &mut self.save_toast,
                            buffer.as_ref(),
                            &mut self.lap_store,
                            &self.coach_status,
                            &self.coach_tips,
                        );
                    });
                    // Re-derive parsed colors in case the color pickers changed them.
                    self.parsed_colors = ParsedColors::from_scheme(&self.settings.colors);
                    if self.running && self.poller.is_none() {
                        self.start();
                    }
                }
            });

        // ── Phase plot viewport ───────────────────────────────────────────────
        if self.settings.graph.phase_plot_open {
            let buffer_arc = self.buffer.clone();
            let graph_settings = self.settings.graph.clone();
            let colors = self.parsed_colors.clone();
            let opacity = self.settings.overlay.opacity;
            let max_steering_angle = self.max_steering_angle;

            let close_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let close_arc = close_flag.clone();

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("phase_plot"),
                egui::ViewportBuilder::default()
                    .with_title("Brake / Steer")
                    .with_inner_size([240.0, 240.0])
                    .with_transparent(true)
                    .with_decorations(false)
                    .with_window_level(egui::WindowLevel::AlwaysOnTop),
                |ctx, _class| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            let size = ui.available_size();
                            let closed = crate::renderer::PhasePlot::new(
                                Some(&*buffer_arc),
                                &graph_settings,
                                &colors,
                                opacity,
                                max_steering_angle,
                            )
                            .show(ui, size);
                            if closed {
                                close_arc.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                        });
                    // Drag the window by clicking anywhere except the close button
                    // (close button is 20×20, centered at top-right corner offset 14, 12).
                    // NOTE: viewport_rect() must be called *outside* the ctx.input() closure —
                    // both acquire egui's internal Context lock and nesting them deadlocks.
                    let vp = ctx.viewport_rect();
                    let close_center = egui::Pos2::new(vp.max.x - 14.0, vp.min.y + 12.0);
                    if ctx.input(|i| {
                        let pressed = i.pointer.button_pressed(egui::PointerButton::Primary);
                        let on_close = i.pointer.interact_pos().is_some_and(|p| {
                            (p.x - close_center.x).hypot(p.y - close_center.y) < 12.0
                        });
                        pressed && !on_close
                    }) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                },
            );

            if close_flag.load(std::sync::atomic::Ordering::Relaxed) {
                self.settings.graph.phase_plot_open = false;
                let _ = self.settings.save_to_config_path();
            }
        }

        // ── Lap comparison viewport ───────────────────────────────────────────
        if self.settings.graph.lap_comparison_open {
            let reference = self.lap_store.reference_lap.clone();
            let current = self.lap_store.current_lap().to_vec();
            let current_track_pos = self
                .lap_store
                .current_lap()
                .last()
                .map(|p| p.track_position)
                .unwrap_or(0.0);
            let colors = self.parsed_colors.clone();
            let opacity = self.settings.overlay.opacity;

            let close_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let close_arc = close_flag.clone();

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("lap_comparison"),
                egui::ViewportBuilder::default()
                    .with_title("Lap Comparison")
                    .with_inner_size([380.0, 260.0])
                    .with_transparent(true)
                    .with_decorations(false)
                    .with_window_level(egui::WindowLevel::AlwaysOnTop),
                |ctx, _class| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            let size = ui.available_size();
                            let closed = crate::renderer::LapComparison::new(
                                reference.as_ref(),
                                &current,
                                current_track_pos,
                                &colors,
                                opacity,
                            )
                            .show(ui, size);
                            if closed {
                                close_arc.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                        });
                    let vp = ctx.viewport_rect();
                    let close_center = egui::Pos2::new(vp.max.x - 14.0, vp.min.y + 12.0);
                    if ctx.input(|i| {
                        let pressed = i.pointer.button_pressed(egui::PointerButton::Primary);
                        let on_close = i.pointer.interact_pos().is_some_and(|p| {
                            (p.x - close_center.x).hypot(p.y - close_center.y) < 12.0
                        });
                        pressed && !on_close
                    }) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                },
            );

            if close_flag.load(std::sync::atomic::Ordering::Relaxed) {
                self.settings.graph.lap_comparison_open = false;
                let _ = self.settings.save_to_config_path();
            }
        }
    }
}

// ── Telemetry layout ─────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn draw_telemetry(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    colors: &ParsedColors,
    buffer: Option<&Arc<crate::core::TelemetryBuffer>>,
    current_steering: f32,
    max_steering_angle: f32,
    a: u8,
    cap_r: f32,
    reference_lap: Option<&[crate::core::lap_store::LapPoint]>,
) {
    let opacity = settings.overlay.opacity;
    let available = ui.available_rect_before_wrap();

    let latest = buffer.and_then(|b| b.latest());
    let throttle = latest.as_ref().map(|p| p.telemetry.throttle).unwrap_or(0.0);
    let brake = latest.as_ref().map(|p| p.telemetry.brake).unwrap_or(0.0);
    let clutch = latest.as_ref().map(|p| p.telemetry.clutch).unwrap_or(0.0);
    let abs_on = latest.as_ref().map(|p| p.abs_active).unwrap_or(false);
    let tc_on = latest
        .as_ref()
        .map(|p| p.telemetry.tc_active)
        .unwrap_or(false);
    let gear = latest.as_ref().map(|p| p.telemetry.gear).unwrap_or(0);
    let speed_ms = latest.as_ref().map(|p| p.telemetry.speed).unwrap_or(0.0);

    let bar_gap = 4.0_f32;
    let gap = 8.0_f32;

    // Reserve space for track strip at bottom when enabled.
    let strip_total = if settings.graph.show_track_strip {
        STRIP_H + STRIP_GAP
    } else {
        0.0
    };
    let content_h = available.height() - strip_total;

    // Wheel column: height-derived but capped so it never crowds the graph
    let wheel_col_w = ((cap_r - 2.0) * 2.0).min(available.width() * 0.30);

    // Bar width scales with height so bars stay proportional when the widget is short
    let bar_w = (content_h * 0.28).clamp(12.0, 22.0);
    let bars_col_w = bar_w * 3.0 + bar_gap * 2.0;

    let graph_w = (available.width() - bars_col_w - wheel_col_w - gap * 2.0).max(40.0);
    let graph_h = content_h;

    // No data arriving? Show overlay on graph area.
    let is_waiting = latest
        .as_ref()
        .is_none_or(|p| p.captured_at.elapsed().as_secs_f32() > 2.0);
    let graph_rect = egui::Rect::from_min_size(available.min, egui::vec2(graph_w, graph_h));

    ui.spacing_mut().item_spacing.x = 0.0;
    ui.horizontal(|ui| {
        // ── Trace graph ──────────────────────────────────────────────────────
        crate::renderer::TraceGraph::new(buffer.map(|v| &**v), &settings.graph, colors, opacity)
            .show(ui, egui::vec2(graph_w, graph_h));

        // Gap between graph and bars
        ui.allocate_exact_size(egui::vec2(gap, content_h), egui::Sense::hover());

        // ── Pedal bars ───────────────────────────────────────────────────────
        let (bars_rect, _) =
            ui.allocate_exact_size(egui::vec2(bars_col_w, content_h), egui::Sense::hover());
        let p = ui.painter();

        let is_braking = brake > 0.01;
        let is_turning = current_steering.abs() > settings.graph.trail_brake_threshold;
        let brake_color = match (is_braking, abs_on && settings.graph.show_abs, is_turning) {
            (true, true, true) if settings.graph.show_abs_cornering => colors.abs_cornering,
            (true, true, _) => colors.abs_active,
            (true, false, true) if settings.graph.show_trail_brake => colors.trail_brake,
            _ => colors.brake,
        };

        let throttle_color = if tc_on && settings.graph.show_tc {
            colors.tc_active
        } else {
            colors.throttle
        };

        let specs: &[(f32, egui::Color32)] = &[
            (clutch, colors.clutch),
            (brake, brake_color),
            (throttle, throttle_color),
        ];

        let label_h = 16.0_f32;
        let bar_labels = ["C", "B", "T"];
        for (i, (value, color)) in specs.iter().enumerate() {
            let x = bars_rect.min.x + i as f32 * (bar_w + bar_gap);
            let top = bars_rect.min.y + label_h + 2.0;
            let bottom = bars_rect.max.y - 4.0;
            let h = bottom - top;

            // Percentage label above the track
            p.text(
                egui::pos2(x + bar_w / 2.0, bars_rect.min.y + label_h / 2.0),
                egui::Align2::CENTER_CENTER,
                format!("{:.0}%", value * 100.0),
                egui::FontId::monospace(10.0),
                with_alpha(LABEL_MID, a),
            );

            let track = egui::Rect::from_min_size(egui::pos2(x, top), egui::vec2(bar_w, h));

            // Track
            p.rect_filled(
                track,
                3.0,
                egui::Color32::from_rgba_unmultiplied(8, 8, 8, (a as f32 * 0.95) as u8),
            );
            p.rect_stroke(
                track,
                3.0,
                egui::Stroke::new(0.5, with_alpha(BORDER, a)),
                egui::StrokeKind::Middle,
            );

            // 50% tick mark
            let mid_y = top + h * 0.5;
            p.line_segment(
                [egui::pos2(x, mid_y), egui::pos2(x + bar_w * 0.4, mid_y)],
                egui::Stroke::new(0.5, with_alpha(LABEL_DIM, a)),
            );

            // Fill
            if *value > 0.005 {
                let fill_h = (h * value).max(2.0);
                p.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(x, bottom - fill_h),
                        egui::vec2(bar_w, fill_h),
                    ),
                    3.0,
                    egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a),
                );
            }

            // C / B / T label at bottom of bar
            p.text(
                egui::pos2(x + bar_w / 2.0, bottom - 7.0),
                egui::Align2::CENTER_CENTER,
                bar_labels[i],
                egui::FontId::monospace(8.0),
                with_alpha(LABEL_DIM, a),
            );
        }

        // Gap between bars and steering wheel
        ui.allocate_exact_size(egui::vec2(gap, content_h), egui::Sense::hover());

        // ── Steering wheel ──────────────────────────────────────────────────
        let (wheel_rect, _) =
            ui.allocate_exact_size(egui::vec2(wheel_col_w, content_h), egui::Sense::hover());
        // Center the wheel in the cap — vertically centred, horizontally at cap centre
        let center = wheel_rect.center();
        // Fit inside the cap with margin for stroke (thickness ≈ radius * 0.28)
        let wheel_radius = (wheel_col_w / 2.0 * 0.82).max(10.0);

        crate::renderer::SteeringWheel::draw(
            ui.painter(),
            center,
            wheel_radius,
            current_steering,
            max_steering_angle,
            opacity,
        );

        // Gear — large, centred inside the ring
        let gear_str = match gear {
            -1 => "R".to_string(),
            0 => "N".to_string(),
            g => g.to_string(),
        };
        ui.painter().text(
            egui::pos2(center.x, center.y - wheel_radius * 0.32),
            egui::Align2::CENTER_CENTER,
            gear_str,
            egui::FontId::monospace((wheel_radius * 0.68).max(10.0)),
            with_alpha(egui::Color32::WHITE, a),
        );

        // Speed — click anywhere on it to toggle kph/mph
        let speed_val = if settings.graph.speed_mph {
            speed_ms * 2.237
        } else {
            speed_ms * 3.6
        };
        let unit_str = if settings.graph.speed_mph {
            "mph"
        } else {
            "kph"
        };
        let speed_pos = egui::pos2(center.x, center.y + wheel_radius * 0.42);
        let speed_rect = egui::Rect::from_center_size(
            speed_pos,
            egui::vec2(wheel_radius * 1.2, wheel_radius * 0.5),
        );
        let speed_resp = ui.allocate_rect(speed_rect, egui::Sense::click());
        if speed_resp.clicked() {
            settings.graph.speed_mph = !settings.graph.speed_mph;
        }
        let speed_font_size = (wheel_radius * 0.42).max(9.0);
        let unit_font_size = (wheel_radius * 0.28).max(8.0);
        // Unit label above the number with a bit of breathing room
        ui.painter().text(
            egui::pos2(center.x, speed_pos.y - speed_font_size * 0.80),
            egui::Align2::CENTER_CENTER,
            unit_str,
            egui::FontId::monospace(unit_font_size),
            with_alpha(LABEL_DIM, a),
        );
        ui.painter().text(
            speed_pos,
            egui::Align2::CENTER_CENTER,
            format!("{:.0}", speed_val),
            egui::FontId::monospace(speed_font_size),
            with_alpha(LABEL_MID, a),
        );
    });

    // ── Track position strip ─────────────────────────────────────────────────
    if settings.graph.show_track_strip {
        let current_track_pos = latest
            .as_ref()
            .map(|p| p.telemetry.track_position)
            .unwrap_or(0.0);
        let strip_rect = egui::Rect::from_min_size(
            egui::pos2(available.min.x, available.max.y - STRIP_H),
            egui::vec2(available.width(), STRIP_H),
        );
        draw_track_strip(
            ui.painter(),
            strip_rect,
            buffer,
            current_track_pos,
            &settings.graph,
            colors,
            a,
            reference_lap,
        );
    }

    if is_waiting {
        ui.painter().text(
            graph_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for game…",
            egui::FontId::monospace(11.0),
            with_alpha(LABEL_DIM, a),
        );
    }
}

// ── Track position strip ──────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn draw_track_strip(
    painter: &egui::Painter,
    rect: egui::Rect,
    buffer: Option<&Arc<crate::core::TelemetryBuffer>>,
    current_pos: f32,
    settings: &crate::config::GraphSettings,
    colors: &ParsedColors,
    a: u8,
    reference_lap: Option<&[crate::core::lap_store::LapPoint]>,
) {
    // Background
    painter.rect_filled(
        rect,
        2.0,
        egui::Color32::from_rgba_unmultiplied(8, 8, 8, (a as f32 * 0.9) as u8),
    );
    painter.rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(0.5, with_alpha(BORDER, a)),
        egui::StrokeKind::Middle,
    );

    // ── Reference lap ghost ─────────────────────────────────────────────────
    // Bin reference points by pixel column and take the max brake/throttle
    // per column, which avoids aliasing when many points map to the same pixel.
    if let Some(ref_lap) = reference_lap {
        let w = rect.width().max(1.0) as usize;
        let mut brake_bins = vec![0.0_f32; w];
        let mut throttle_bins = vec![0.0_f32; w];

        for pt in ref_lap {
            let bin = ((pt.track_position.clamp(0.0, 1.0) * w as f32) as usize).min(w - 1);
            brake_bins[bin] = brake_bins[bin].max(pt.brake);
            throttle_bins[bin] = throttle_bins[bin].max(pt.throttle);
        }

        let inner_h = rect.height() - 2.0;
        let ghost_opacity = a as f32 * 0.38; // dim — reference sits behind live data

        let [br, bg, bb, _] = colors.brake.to_array();
        let [tr, tg, tb, _] = colors.throttle.to_array();

        for col in 0..w {
            let x = rect.min.x + col as f32 + 0.5;

            // Brake: from bottom up
            if brake_bins[col] > 0.02 {
                let h = (brake_bins[col] * inner_h).max(1.0);
                let alpha = (ghost_opacity * brake_bins[col]).min(255.0) as u8;
                painter.line_segment(
                    [
                        egui::pos2(x, rect.max.y - 1.0),
                        egui::pos2(x, rect.max.y - 1.0 - h),
                    ],
                    egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_unmultiplied(br, bg, bb, alpha),
                    ),
                );
            }

            // Throttle: from top down
            if throttle_bins[col] > 0.02 {
                let h = (throttle_bins[col] * inner_h).max(1.0);
                let alpha = (ghost_opacity * throttle_bins[col]).min(255.0) as u8;
                painter.line_segment(
                    [
                        egui::pos2(x, rect.min.y + 1.0),
                        egui::pos2(x, rect.min.y + 1.0 + h),
                    ],
                    egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_unmultiplied(tr, tg, tb, alpha),
                    ),
                );
            }
        }
    }

    // Live brake and throttle blips from recent history.
    if let Some(buf) = buffer {
        let points = buf.get_points();
        let now = std::time::Instant::now();
        let window_secs = settings.window_seconds as f32;
        let window_dur = std::time::Duration::from_secs_f64(settings.window_seconds);
        let inner_h = rect.height() - 2.0;

        for pt in points
            .iter()
            .filter(|p| now.duration_since(p.captured_at) <= window_dur)
        {
            let age = now.duration_since(pt.captured_at).as_secs_f32();
            let freshness = (1.0 - age / window_secs).clamp(0.0, 1.0);
            let x = rect.min.x + pt.telemetry.track_position * rect.width();

            // Brake: from bottom up
            let brake_intensity = pt.telemetry.brake * freshness;
            if brake_intensity > 0.02 {
                let color = if pt.abs_active && settings.show_abs {
                    colors.abs_active
                } else {
                    colors.brake
                };
                let [r, g, b, ca] = color.to_array();
                let alpha = ((ca as f32) * (a as f32 / 255.0) * brake_intensity).min(255.0) as u8;
                let h = (pt.telemetry.brake * inner_h).max(1.0);
                painter.line_segment(
                    [
                        egui::pos2(x, rect.max.y - 1.0),
                        egui::pos2(x, rect.max.y - 1.0 - h),
                    ],
                    egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(r, g, b, alpha)),
                );
            }

            // Throttle: from top down
            let throttle_intensity = pt.telemetry.throttle * freshness;
            if throttle_intensity > 0.02 {
                let [r, g, b, ca] = colors.throttle.to_array();
                let alpha =
                    ((ca as f32) * (a as f32 / 255.0) * throttle_intensity).min(255.0) as u8;
                let h = (pt.telemetry.throttle * inner_h).max(1.0);
                painter.line_segment(
                    [
                        egui::pos2(x, rect.min.y + 1.0),
                        egui::pos2(x, rect.min.y + 1.0 + h),
                    ],
                    egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(r, g, b, alpha)),
                );
            }
        }
    }

    // Current position cursor — bright white vertical line.
    let cx = rect.min.x + current_pos.clamp(0.0, 1.0) * rect.width();
    painter.line_segment(
        [egui::pos2(cx, rect.min.y), egui::pos2(cx, rect.max.y)],
        egui::Stroke::new(2.0, with_alpha(egui::Color32::WHITE, a)),
    );
}

// ── Config panel ─────────────────────────────────────────────────────────────

fn draw_config(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    running: &mut bool,
    save_toast: &mut Option<std::time::Instant>,
    buffer: Option<&Arc<crate::core::TelemetryBuffer>>,
    lap_store: &mut crate::core::LapStore,
    coach_status: &CoachStatus,
    coach_tips: &VecDeque<CoachTip>,
) {
    // Ensure all widgets (sliders, dropdowns, colour pickers) use dark styling
    // regardless of the OS theme reported by the platform layer.
    *ui.visuals_mut() = egui::Visuals::dark();
    ui.visuals_mut().override_text_color = Some(egui::Color32::from_gray(210));

    let is_live = buffer
        .and_then(|b| b.latest())
        .is_some_and(|p| p.captured_at.elapsed().as_secs_f32() <= 2.0);

    // Status dot + label + stop/start + save (right-aligned) — all inline
    ui.horizontal(|ui| {
        let (dot_color, status_text): (egui::Color32, &str) = if !*running {
            (egui::Color32::from_gray(60), "STOPPED")
        } else if is_live {
            (egui::Color32::from_rgb(60, 200, 80), "LIVE")
        } else {
            (egui::Color32::from_rgb(220, 140, 40), "CONNECTING")
        };
        let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(dot_rect.center(), 4.0, dot_color);
        ui.label(
            egui::RichText::new(status_text)
                .size(10.0)
                .monospace()
                .color(dot_color),
        );
        ui.add_space(8.0);
        let run_label = if *running { "■  Stop" } else { "▶  Start" };
        if ui.add(styled_button(run_label)).clicked() {
            *running = !*running;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(styled_button("Save")).clicked() {
                if let Err(e) = settings.save_to_config_path() {
                    tracing::error!("{e}");
                } else {
                    *save_toast = Some(std::time::Instant::now());
                }
            }
        });
    });
    // Save toast
    if let Some(t) = *save_toast {
        let elapsed = t.elapsed().as_secs_f32();
        if elapsed < 2.0 {
            let fade = if elapsed > 1.5 {
                1.0 - (elapsed - 1.5) / 0.5
            } else {
                1.0
            };
            let ta = (fade * 255.0) as u8;
            ui.label(
                egui::RichText::new("Saved")
                    .size(10.0)
                    .monospace()
                    .color(egui::Color32::from_rgba_unmultiplied(100, 220, 100, ta)),
            );
            ui.ctx().request_repaint();
        } else {
            *save_toast = None;
        }
    }

    // ── Game ─────────────────────────────────────────────────────────────────
    section_header(ui, "GAME");
    egui::ComboBox::from_id_salt("plugin")
        .width(ui.available_width())
        .selected_text(plugin_display_name(&settings.collector.plugin))
        .show_ui(ui, |ui| {
            for (id, name) in crate::plugins::plugin_entries() {
                ui.selectable_value(&mut settings.collector.plugin, id.to_string(), *name);
            }
        });

    // ── Display ──────────────────────────────────────────────────────────────
    section_header(ui, "DISPLAY");
    ui.checkbox(&mut settings.graph.show_legend, "Show legend");
    ui.checkbox(&mut settings.graph.show_track_strip, "Show track strip");
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Speed unit")
                .size(11.0)
                .color(LABEL_MID),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            egui::ComboBox::from_id_salt("speed_unit")
                .selected_text(if settings.graph.speed_mph {
                    "mph"
                } else {
                    "kph"
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut settings.graph.speed_mph, false, "kph");
                    ui.selectable_value(&mut settings.graph.speed_mph, true, "mph");
                });
        });
    });
    ui.add_space(4.0);
    slider_row(ui, "Opacity", &mut settings.overlay.opacity, 0.1..=1.0, "");
    slider_row(
        ui,
        "Trace width",
        &mut settings.graph.line_width,
        0.5..=5.0,
        " px",
    );
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Time window")
                .size(11.0)
                .color(LABEL_MID),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut w = settings.graph.window_seconds as f32;
            if ui
                .add(
                    egui::Slider::new(&mut w, 3.0..=MAX_DISPLAY_WINDOW_SECS)
                        .suffix(" s")
                        .show_value(true),
                )
                .changed()
            {
                settings.graph.window_seconds = w as f64;
            }
        });
    });

    // ── Traces ───────────────────────────────────────────────────────────────
    section_header(ui, "TRACES");
    trace_section(
        ui,
        "THROTTLE",
        &mut settings.graph.show_throttle,
        &mut settings.colors.throttle,
    );
    ui.indent("tc_indent", |ui| {
        trace_section(
            ui,
            "TC",
            &mut settings.graph.show_tc,
            &mut settings.colors.tc_active,
        );
    });
    trace_section(
        ui,
        "BRAKE",
        &mut settings.graph.show_brake,
        &mut settings.colors.brake,
    );
    ui.indent("abs_indent", |ui| {
        trace_section(
            ui,
            "ABS",
            &mut settings.graph.show_abs,
            &mut settings.colors.abs_active,
        );
    });
    trace_section(
        ui,
        "CLUTCH",
        &mut settings.graph.show_clutch,
        &mut settings.colors.clutch,
    );
    trace_section(
        ui,
        "SPEED",
        &mut settings.graph.show_speed,
        &mut settings.colors.speed,
    );

    // ── Trail braking ─────────────────────────────────────────────────────────
    section_header(ui, "TRAIL BRAKING");
    if settings.graph.show_brake {
        slider_row(
            ui,
            "Steer threshold",
            &mut settings.graph.trail_brake_threshold,
            0.5..=45.0,
            "°",
        );
        trail_color_section(
            ui,
            "TRAIL BRAKING",
            "Braking while turning, before ABS",
            &mut settings.graph.show_trail_brake,
            &mut settings.colors.trail_brake,
        );
        trail_color_section(
            ui,
            "ABS WHILE TURNING",
            "ABS activated mid-corner — indicates over-braking",
            &mut settings.graph.show_abs_cornering,
            &mut settings.colors.abs_cornering,
        );
    }
    let phase_label = if settings.graph.phase_plot_open {
        "Hide phase plot"
    } else {
        "Show phase plot"
    };
    if ui.add(styled_button(phase_label)).clicked() {
        settings.graph.phase_plot_open = !settings.graph.phase_plot_open;
    }

    // ── Lap comparison ────────────────────────────────────────────────────────
    section_header(ui, "LAP COMPARISON");
    let cmp_label = if settings.graph.lap_comparison_open {
        "Hide comparison"
    } else {
        "Show comparison"
    };
    if ui.add(styled_button(cmp_label)).clicked() {
        settings.graph.lap_comparison_open = !settings.graph.lap_comparison_open;
    }
    ui.add_space(4.0);
    let ref_status = match &lap_store.reference_lap {
        Some(pts) => format!("Ref: {} pts", pts.len()),
        None => "No reference lap".to_string(),
    };
    ui.label(egui::RichText::new(ref_status).size(10.0).color(LABEL_DIM));
    ui.horizontal(|ui| {
        if ui.add(styled_button("Set Ref")).clicked() {
            lap_store.set_current_as_reference();
        }
        if ui.add(styled_button("Clear")).clicked() {
            lap_store.clear_reference();
        }
    });

    // ── AI Coach ─────────────────────────────────────────────────────────────
    section_header(ui, "AI COACH");

    ui.horizontal(|ui| {
        ui.checkbox(&mut settings.coach.enabled, "Enable coach");
    });

    if settings.coach.enabled {
        ui.add_space(4.0);

        // Status row
        ui.horizontal(|ui| {
            let (dot_col, label) = if coach_status.has_track_map {
                (
                    egui::Color32::from_rgb(60, 200, 80),
                    format!("{} corners mapped", coach_status.corner_count),
                )
            } else {
                (
                    egui::Color32::from_rgb(220, 140, 40),
                    "Learning track…".to_string(),
                )
            };
            let (dot_rect, _) =
                ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(dot_rect.center(), 4.0, dot_col);
            ui.label(
                egui::RichText::new(&label)
                    .size(10.0)
                    .monospace()
                    .color(dot_col),
            );
        });

        if coach_status.has_reference {
            let ref_label = if let Some(ms) = coach_status.best_lap_ms {
                let mins = ms / 60_000;
                let secs = (ms % 60_000) as f32 / 1000.0;
                format!("Reference: {:02}:{:06.3}", mins, secs)
            } else {
                "Reference: set".to_string()
            };
            ui.label(egui::RichText::new(ref_label).size(10.0).color(LABEL_MID));
        } else {
            ui.label(
                egui::RichText::new("Reference: none yet")
                    .size(10.0)
                    .color(LABEL_DIM),
            );
        }

        ui.add_space(4.0);

        // Cooldown slider
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Tip cooldown")
                    .size(10.0)
                    .color(LABEL_MID),
            );
            ui.add(
                egui::Slider::new(&mut settings.coach.cooldown_secs, 5..=60)
                    .suffix(" s")
                    .clamping(egui::SliderClamping::Always),
            );
        });

        // Reference lap strategy
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Reference").size(10.0).color(LABEL_MID));
            egui::ComboBox::from_id_salt("ref_strategy")
                .selected_text(match settings.coach.reference_lap_strategy {
                    ReferenceLapStrategy::Best => "Best lap",
                    ReferenceLapStrategy::Last => "Last lap",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.coach.reference_lap_strategy,
                        ReferenceLapStrategy::Best,
                        "Best lap",
                    );
                    ui.selectable_value(
                        &mut settings.coach.reference_lap_strategy,
                        ReferenceLapStrategy::Last,
                        "Last lap",
                    );
                });
        });

        // Recent tips
        if !coach_tips.is_empty() {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("RECENT TIPS")
                    .size(9.0)
                    .color(LABEL_DIM),
            );
            for tip in coach_tips.iter().rev() {
                let age = tip.generated_at.elapsed().as_secs();
                let age_str = if age < 60 {
                    format!("{}s ago", age)
                } else {
                    format!("{}m ago", age / 60)
                };
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&age_str)
                            .size(8.0)
                            .color(LABEL_DIM)
                            .monospace(),
                    );
                    ui.label(egui::RichText::new(&tip.text).size(9.0).color(LABEL_MID));
                });
            }
        }

        // ── TTS voice ─────────────────────────────────────────────────────────
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("VOICE (TTS)")
                .size(9.0)
                .monospace()
                .color(LABEL_DIM),
        );
        ui.add_space(2.0);

        let (dot_col, status_text) = if coach_status.tts_active {
            (egui::Color32::from_rgb(60, 200, 80), "Speaking")
        } else if settings.coach.tts_enabled {
            if coach_status.tts_error.is_some() {
                (ACCENT_RED, "Error — see below")
            } else {
                (egui::Color32::from_rgb(220, 140, 40), "Starting…")
            }
        } else {
            (LABEL_DIM, "Disabled")
        };
        ui.horizontal(|ui| {
            let (dot_rect, _) =
                ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(dot_rect.center(), 4.0, dot_col);
            ui.label(
                egui::RichText::new(status_text)
                    .size(10.0)
                    .monospace()
                    .color(dot_col),
            );
        });
        if let Some(ref err) = coach_status.tts_error {
            ui.label(
                egui::RichText::new(format!("⚠  {err}"))
                    .size(9.0)
                    .color(ACCENT_RED),
            );
        }
        ui.checkbox(&mut settings.coach.tts_enabled, "Enable spoken tips");

        ui.add_space(4.0);
        if ui.add(styled_button("Open data folder")).clicked() {
            open_in_file_manager(&settings.coach.data_dir());
        }
    }

    // ── Logs ─────────────────────────────────────────────────────────────────
    section_header(ui, "LOGS");
    if ui.add(styled_button("Open log folder")).clicked() {
        if let Some(dir) = AppSettings::config_dir() {
            open_in_file_manager(&dir);
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn with_alpha(c: egui::Color32, a: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn section_header(ui: &mut egui::Ui, label: &str) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(9.0)
                .monospace()
                .color(LABEL_DIM),
        );
        let y = ui.next_widget_position().y + 5.0;
        let x0 = ui.next_widget_position().x;
        let x1 = ui.max_rect().max.x;
        if x1 > x0 {
            ui.painter().line_segment(
                [egui::pos2(x0 + 4.0, y), egui::pos2(x1, y)],
                egui::Stroke::new(0.5, BORDER),
            );
        }
    });
    ui.add_space(4.0);
}

fn slider_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(11.0).color(LABEL_MID));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::Slider::new(value, range)
                    .suffix(suffix)
                    .show_value(true),
            );
        });
    });
}

fn styled_button(label: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(label.to_owned()).size(11.0))
        .fill(egui::Color32::from_rgb(38, 38, 38))
}

/// Sub-section with a small header, enabled checkbox, and colour swatch.
fn trace_section(ui: &mut egui::Ui, label: &str, enabled: &mut bool, hex: &mut String) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let label_color = if *enabled { LABEL_MID } else { LABEL_DIM };
        ui.label(
            egui::RichText::new(label)
                .size(9.0)
                .monospace()
                .color(label_color),
        );
        let y = ui.next_widget_position().y + 5.0;
        let x0 = ui.next_widget_position().x + 4.0;
        let x1 = ui.max_rect().max.x;
        if x1 > x0 {
            ui.painter().line_segment(
                [egui::pos2(x0, y), egui::pos2(x1, y)],
                egui::Stroke::new(0.5, BORDER),
            );
        }
    });
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.checkbox(
            enabled,
            egui::RichText::new("Enabled").size(11.0).color(LABEL_MID),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut color = AppSettings::parse_color(hex);
            if color_edit_button_srgba(ui, &mut color, Alpha::Opaque).changed() {
                *hex = format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b());
            }
        });
    });
}

/// Like `trace_section` but with a description line explaining the color's meaning.
fn trail_color_section(
    ui: &mut egui::Ui,
    label: &str,
    description: &str,
    enabled: &mut bool,
    hex: &mut String,
) {
    ui.add_space(4.0);
    // Sub-section header with separator line — same style as trace_section
    ui.horizontal(|ui| {
        let label_color = if *enabled { LABEL_MID } else { LABEL_DIM };
        ui.label(
            egui::RichText::new(label)
                .size(9.0)
                .monospace()
                .color(label_color),
        );
        let y = ui.next_widget_position().y + 5.0;
        let x0 = ui.next_widget_position().x + 4.0;
        let x1 = ui.max_rect().max.x;
        if x1 > x0 {
            ui.painter().line_segment(
                [egui::pos2(x0, y), egui::pos2(x1, y)],
                egui::Stroke::new(0.5, BORDER),
            );
        }
    });
    ui.add_space(2.0);
    ui.label(egui::RichText::new(description).size(10.0).color(LABEL_DIM));
    ui.horizontal(|ui| {
        ui.checkbox(
            enabled,
            egui::RichText::new("Enabled").size(11.0).color(LABEL_MID),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut color = AppSettings::parse_color(hex);
            if color_edit_button_srgba(ui, &mut color, Alpha::Opaque).changed() {
                *hex = format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b());
            }
        });
    });
}

fn open_in_file_manager(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

fn plugin_display_name(id: &str) -> &str {
    crate::plugins::plugin_entries()
        .iter()
        .find(|(entry_id, _)| *entry_id == id)
        .map(|(_, name)| *name)
        .unwrap_or(id)
}

// ── Stadium shape ─────────────────────────────────────────────────────────────
// Flat-left rectangle with a semicircular right cap.
// cap_r ≤ rect.height()/2 keeps the shape convex.
fn stadium_path(rect: egui::Rect, left_r: f32, cap_r: f32) -> Vec<egui::Pos2> {
    use std::f32::consts::{FRAC_PI_2, PI};
    let cap_r = cap_r.min(rect.height() / 2.0);
    let cx = rect.max.x - cap_r; // arc centre x
    let cy = rect.center().y; // arc centre y
    let cap_ty = cy - cap_r; // arc top y
    let cap_by = cy + cap_r; // arc bottom y

    let mut pts = Vec::with_capacity(100);
    let corners = 8usize;
    let arc_pts = 48usize;

    // Top-left corner (π → 3π/2)
    let tl = egui::pos2(rect.min.x + left_r, rect.min.y + left_r);
    for i in 0..=corners {
        let a = PI + (i as f32 / corners as f32) * FRAC_PI_2;
        pts.push(egui::pos2(tl.x + left_r * a.cos(), tl.y + left_r * a.sin()));
    }

    // Top edge → right shoulder
    pts.push(egui::pos2(cx, rect.min.y));
    if cap_ty > rect.min.y {
        pts.push(egui::pos2(cx, cap_ty));
    }

    // Right semicircle (-π/2 → π/2)
    for i in 0..=arc_pts {
        let a = -FRAC_PI_2 + (i as f32 / arc_pts as f32) * PI;
        pts.push(egui::pos2(cx + cap_r * a.cos(), cy + cap_r * a.sin()));
    }

    // Right shoulder → bottom edge
    if cap_by < rect.max.y {
        pts.push(egui::pos2(cx, rect.max.y));
    }
    pts.push(egui::pos2(rect.min.x + left_r, rect.max.y));

    // Bottom-left corner (π/2 → π)
    let bl = egui::pos2(rect.min.x + left_r, rect.max.y - left_r);
    for i in 0..=corners {
        let a = FRAC_PI_2 + (i as f32 / corners as f32) * FRAC_PI_2;
        pts.push(egui::pos2(bl.x + left_r * a.cos(), bl.y + left_r * a.sin()));
    }

    pts
}
