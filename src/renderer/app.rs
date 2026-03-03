//! Main application

use eframe::egui;
use crate::config::AppSettings;
use crate::core::DataCollector;
use std::sync::{Arc, Mutex};
use egui::color_picker::{color_edit_button_srgba, Alpha};

// ── Palette ──────────────────────────────────────────────────────────────────
const BAR_BG:      egui::Color32 = egui::Color32::from_rgb(13, 13, 13);
const CARD_BG:     egui::Color32 = egui::Color32::from_rgb(16, 16, 16);
const BORDER:      egui::Color32 = egui::Color32::from_rgb(26, 26, 26);
const LABEL_DIM:   egui::Color32 = egui::Color32::from_rgb(90, 90, 90);
const LABEL_MID:   egui::Color32 = egui::Color32::from_rgb(140, 140, 140);
const ACCENT_RED:  egui::Color32 = egui::Color32::from_rgb(220, 45, 45);

pub struct SimTraceApp {
    settings: AppSettings,
    collector: Option<Arc<Mutex<DataCollector>>>,
    current_steering: f32,
    running: bool,
    config_open: bool,
    /// 0.0 = fully hidden, 1.0 = fully visible
    bar_alpha: f32,
    /// Tracks which plugin is currently active so we can detect dropdown changes
    active_plugin: String,
}

impl SimTraceApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals {
            panel_fill: egui::Color32::TRANSPARENT,
            window_fill: egui::Color32::TRANSPARENT,
            ..egui::Visuals::dark()
        });

        let (settings, _) = load_settings();
        let active_plugin = settings.collector.plugin.clone();
        Self {
            settings,
            collector: None,
            current_steering: 0.0,
            running: true,
            config_open: false,
            bar_alpha: 1.0,
            active_plugin,
        }
    }

    fn start(&mut self) {
        if self.collector.is_none() {
            let cfg = crate::core::collector::CollectorConfig {
                update_rate_hz: self.settings.collector.update_rate_hz,
                buffer_window_secs: self.settings.collector.buffer_window_secs.unwrap_or(10),
            };
            self.collector = Some(Arc::new(Mutex::new(DataCollector::new(cfg))));
        }
        self.activate_plugin();
        self.running = true;
    }

    fn activate_plugin(&mut self) {
        let plugin = self.settings.collector.plugin.clone();
        if let Some(c) = &self.collector {
            if let Ok(mut c) = c.lock() {
                let _ = c.activate_plugin(&plugin);
            }
        }
        self.active_plugin = plugin;
    }

    fn stop(&mut self) {
        self.running = false;
    }
}

impl eframe::App for SimTraceApp {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── Poll telemetry ───────────────────────────────────────────────────
        let buffer = if self.running {
            if let Some(collector) = self.collector.clone() {
                if let Ok(mut c) = collector.lock() {
                    c.poll();
                    if let Some(pt) = c.buffer().latest() {
                        self.current_steering = pt.telemetry.steering_angle;
                    }
                }
            }
            self.collector.as_ref().and_then(|c| c.lock().ok().map(|c| c.buffer()))
        } else {
            None
        };

        if self.running && self.collector.is_none() {
            self.start();
        } else if self.settings.collector.plugin != self.active_plugin {
            self.activate_plugin();
        }

        let fps = self.settings.graph.overlay_fps;
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(1.0 / fps as f64));

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                let screen   = ui.max_rect();
                let opacity  = self.settings.overlay.opacity;
                let a        = (opacity * 255.0) as u8;
                let bar_h    = 26.0_f32;
                let pad      = 2.0_f32;

                // ── Hover detection + bar fade ───────────────────────────────
                let hovered = ctx.input(|i| {
                    i.pointer.hover_pos()
                        .map(|p| screen.contains(p))
                        .unwrap_or(false)
                });
                let target = if hovered || self.config_open { 1.0_f32 } else { 0.0_f32 };
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
                ui.painter().rect_filled(bar_rect, egui::Rounding { nw: 5.0, ne: 5.0, sw: 0.0, se: 0.0 }, with_alpha(BAR_BG, ba));

                // Red accent stripe along the top edge of the bar
                ui.painter().line_segment(
                    [bar_rect.min, egui::pos2(bar_rect.max.x, bar_rect.min.y)],
                    egui::Stroke::new(2.0, with_alpha(ACCENT_RED, ba)),
                );
                // Bottom divider
                ui.painter().line_segment(
                    [egui::pos2(bar_rect.min.x, bar_rect.max.y),
                     egui::pos2(bar_rect.max.x, bar_rect.max.y)],
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
                        [egui::pos2(gx - 12.0, gy + dy), egui::pos2(gx + 12.0, gy + dy)],
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
                        close_rect, 4.0,
                        egui::Color32::from_rgba_unmultiplied(80, 30, 30, ba),
                    );
                }
                ui.painter().text(
                    close_rect.center(), egui::Align2::CENTER_CENTER,
                    "×", egui::FontId::proportional(14.0),
                    with_alpha(LABEL_MID, ba),
                );
                if close_resp.clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }

                // Gear button — left of close button
                let gear_rect = egui::Rect::from_center_size(
                    egui::pos2(bar_rect.max.x - 40.0, bar_rect.center().y),
                    egui::vec2(22.0, 22.0),
                );
                let gear_resp = ui.allocate_rect(gear_rect, egui::Sense::click());
                if self.config_open || gear_resp.hovered() {
                    ui.painter().rect_filled(
                        gear_rect, 4.0,
                        egui::Color32::from_rgba_unmultiplied(60, 60, 60, ba),
                    );
                }
                ui.painter().text(
                    gear_rect.center(), egui::Align2::CENTER_CENTER,
                    "⚙", egui::FontId::proportional(13.0),
                    with_alpha(if self.config_open { egui::Color32::WHITE } else { LABEL_MID }, ba),
                );
                if gear_resp.clicked() { self.config_open = !self.config_open; }

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

                if self.running {
                    let mut content_ui = ui.child_ui(
                        content_rect.shrink(2.0),
                        egui::Layout::top_down(egui::Align::LEFT),
                        None,
                    );
                    draw_telemetry(
                        &mut content_ui, &mut self.settings,
                        buffer.as_ref(), self.current_steering, a,
                        cap_r,
                    );
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
                                (inner.width()  + delta.x).max(200.0),
                                (inner.height() + delta.y).max(100.0),
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

                // ── Config panel ─────────────────────────────────────────────
                if self.config_open {
                    let panel_w   = 260.0_f32.min(screen.width() - 8.0);
                    let panel_top = screen.min.y + bar_h + 4.0;
                    let panel_rect = egui::Rect::from_min_size(
                        egui::pos2(screen.max.x - panel_w - 4.0, panel_top),
                        egui::vec2(panel_w, (screen.max.y - panel_top - 4.0).max(40.0)),
                    );
                    ui.painter().rect_filled(panel_rect, 6.0, egui::Color32::from_rgb(20, 20, 20));
                    ui.painter().rect_stroke(
                        panel_rect, 6.0,
                        egui::Stroke::new(1.0, BORDER),
                    );
                    let mut child = ui.child_ui(
                        panel_rect.shrink(12.0),
                        egui::Layout::top_down(egui::Align::LEFT),
                        None,
                    );
                    egui::ScrollArea::vertical().show(&mut child, |ui| {
                        draw_config(ui, &mut self.settings, &mut self.running);
                    });
                    if self.running && self.collector.is_none() { self.start(); }
                }
            });
    }
}

// ── Telemetry layout ─────────────────────────────────────────────────────────

fn draw_telemetry(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    buffer: Option<&Arc<crate::core::TelemetryBuffer>>,
    current_steering: f32,
    a: u8,
    cap_r: f32,
) {
    let opacity  = settings.overlay.opacity;
    let available = ui.available_rect_before_wrap();

    let latest   = buffer.and_then(|b| b.latest());
    let throttle = latest.as_ref().map(|p| p.telemetry.throttle).unwrap_or(0.0);
    let brake    = latest.as_ref().map(|p| p.telemetry.brake).unwrap_or(0.0);
    let clutch   = latest.as_ref().map(|p| p.telemetry.clutch).unwrap_or(0.0);
    let abs_on   = latest.as_ref().map(|p| p.abs_active).unwrap_or(false);
    let gear     = latest.as_ref().map(|p| p.telemetry.gear).unwrap_or(0);
    let speed_ms = latest.as_ref().map(|p| p.telemetry.speed).unwrap_or(0.0);

    let bar_w      = 20.0_f32;
    let bar_gap    = 4.0_f32;
    let bars_col_w = bar_w * 3.0 + bar_gap * 2.0;
    let gap        = 8.0_f32; // equal spacing between graph↔bars and bars↔wheel

    // Wheel column matches the stadium cap exactly (cap_r from content_rect, minus shrink)
    let wheel_col_w = (cap_r - 2.0) * 2.0;
    let graph_w     = (available.width() - bars_col_w - wheel_col_w - gap * 2.0).max(40.0);
    let graph_h     = available.height();

    ui.spacing_mut().item_spacing.x = 0.0;
    ui.horizontal(|ui| {
        // ── Trace graph ──────────────────────────────────────────────────────
        crate::renderer::TraceGraph::new_simple(
            buffer.map(|v| &**v), &settings.graph, &settings.colors, opacity,
        )
        .show_simple(ui, egui::vec2(graph_w, graph_h));

        // Gap between graph and bars
        ui.allocate_exact_size(egui::vec2(gap, available.height()), egui::Sense::hover());

        // ── Pedal bars ───────────────────────────────────────────────────────
        let (bars_rect, _) = ui.allocate_exact_size(
            egui::vec2(bars_col_w, available.height()), egui::Sense::hover(),
        );
        let p = ui.painter();

        let brake_color = if abs_on {
            AppSettings::parse_color(&settings.colors.abs_active)
        } else {
            AppSettings::parse_color(&settings.colors.brake)
        };

        let specs: &[(f32, egui::Color32)] = &[
            (clutch,   AppSettings::parse_color(&settings.colors.clutch)),
            (brake,    brake_color),
            (throttle, AppSettings::parse_color(&settings.colors.throttle)),
        ];

        let label_h = 16.0_f32;
        for (i, (value, color)) in specs.iter().enumerate() {
            let x      = bars_rect.min.x + i as f32 * (bar_w + bar_gap);
            let top    = bars_rect.min.y + label_h + 2.0;
            let bottom = bars_rect.max.y - 4.0;
            let h      = bottom - top;

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
            p.rect_filled(track, 3.0, egui::Color32::from_rgba_unmultiplied(8, 8, 8, (a as f32 * 0.95) as u8));
            p.rect_stroke(track, 3.0, egui::Stroke::new(0.5, with_alpha(BORDER, a)));

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
        }

        // Gap between bars and steering wheel
        ui.allocate_exact_size(egui::vec2(gap, available.height()), egui::Sense::hover());

        // ── Steering wheel ───────────────────────────────────────────────────
        let (wheel_rect, _) = ui.allocate_exact_size(
            egui::vec2(wheel_col_w, available.height()), egui::Sense::hover(),
        );
        // Center the wheel in the cap — vertically centred, horizontally at cap centre
        let center       = wheel_rect.center();
        // Fit inside the cap with margin for stroke (thickness ≈ radius * 0.28)
        let wheel_radius = (wheel_col_w / 2.0 * 0.64).max(10.0);

        crate::renderer::SteeringWheel::draw(
            ui.painter(), center, wheel_radius, current_steering, opacity,
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
        let speed_val = if settings.graph.speed_mph { speed_ms * 2.237 } else { speed_ms * 3.6 };
        let unit_str  = if settings.graph.speed_mph { "mph" } else { "kph" };
        let speed_pos  = egui::pos2(center.x, center.y + wheel_radius * 0.42);
        let speed_rect = egui::Rect::from_center_size(speed_pos, egui::vec2(wheel_radius * 1.2, wheel_radius * 0.5));
        let speed_resp = ui.allocate_rect(speed_rect, egui::Sense::click());
        if speed_resp.clicked() { settings.graph.speed_mph = !settings.graph.speed_mph; }
        let speed_font_size = (wheel_radius * 0.42).max(9.0);
        let unit_font_size  = (wheel_radius * 0.28).max(8.0);
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
}

// ── Config panel ─────────────────────────────────────────────────────────────

fn draw_config(ui: &mut egui::Ui, settings: &mut AppSettings, running: &mut bool) {
    // Force readable text on dark panel
    ui.visuals_mut().override_text_color = Some(egui::Color32::from_gray(210));

    // Status dot + label + stop/start + save (right-aligned) — all inline
    ui.horizontal(|ui| {
        let (dot_color, status_text) = if *running {
            (egui::Color32::from_rgb(60, 200, 80), "LIVE")
        } else {
            (egui::Color32::from_gray(60), "STOPPED")
        };
        let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        ui.painter().circle_filled(dot_rect.center(), 4.0, dot_color);
        ui.label(
            egui::RichText::new(status_text)
                .size(10.0).monospace()
                .color(dot_color),
        );
        ui.add_space(8.0);
        let run_label = if *running { "■  Stop" } else { "▶  Start" };
        if ui.add(styled_button(run_label)).clicked() { *running = !*running; }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(styled_button("Save")).clicked() {
                if let Err(e) = save_settings(settings) { tracing::error!("{e}"); }
            }
        });
    });
    // ── Game ─────────────────────────────────────────────────────────────────
    section_header(ui, "GAME");
    egui::ComboBox::from_id_source("plugin")
        .width(ui.available_width())
        .selected_text(plugin_display_name(&settings.collector.plugin))
        .show_ui(ui, |ui| {
            for (id, name) in &[
                ("assetto_competizione", "Assetto Corsa Competizione"),
                ("mock", "Mock (Simulated Data)"),
            ] {
                ui.selectable_value(&mut settings.collector.plugin, id.to_string(), *name);
            }
        });

    // ── Display ──────────────────────────────────────────────────────────────
    section_header(ui, "DISPLAY");
    ui.checkbox(&mut settings.graph.show_legend, "Show legend");
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Speed unit").size(11.0).color(LABEL_MID));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            egui::ComboBox::from_id_source("speed_unit")
                .selected_text(if settings.graph.speed_mph { "mph" } else { "kph" })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut settings.graph.speed_mph, false, "kph");
                    ui.selectable_value(&mut settings.graph.speed_mph, true, "mph");
                });
        });
    });
    ui.add_space(4.0);
    slider_row(ui, "Opacity", &mut settings.overlay.opacity, 0.1..=1.0, "");
    slider_row_int(ui, "FPS", &mut settings.graph.overlay_fps, 10..=120, " fps");

    // ── Colours ──────────────────────────────────────────────────────────────
    section_header(ui, "COLOURS");
    color_row(ui, "Throttle", &mut settings.colors.throttle);
    color_row(ui, "Brake",    &mut settings.colors.brake);
    color_row(ui, "ABS",      &mut settings.colors.abs_active);
    color_row(ui, "Clutch",   &mut settings.colors.clutch);
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn with_alpha(c: egui::Color32, a: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn section_header(ui: &mut egui::Ui, label: &str) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(9.0).monospace().color(LABEL_DIM));
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

fn slider_row(ui: &mut egui::Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, suffix: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(11.0).color(LABEL_MID));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(egui::Slider::new(value, range).suffix(suffix).show_value(true));
        });
    });
}

fn slider_row_int(ui: &mut egui::Ui, label: &str, value: &mut u32, range: std::ops::RangeInclusive<u32>, suffix: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(11.0).color(LABEL_MID));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(egui::Slider::new(value, range).suffix(suffix).show_value(true));
        });
    });
}

fn styled_button(label: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(label.to_owned()).size(11.0))
        .fill(egui::Color32::from_rgb(38, 38, 38))
}

fn color_row(ui: &mut egui::Ui, label: &str, hex: &mut String) {
    ui.horizontal(|ui| {
        let mut color = AppSettings::parse_color(hex);
        if color_edit_button_srgba(ui, &mut color, Alpha::Opaque).changed() {
            *hex = format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b());
        }
        ui.label(egui::RichText::new(label).size(11.0).color(LABEL_MID));
    });
}

fn plugin_display_name(plugin: &str) -> &str {
    match plugin {
        "assetto_competizione" => "Assetto Corsa Competizione",
        "mock" | "test"        => "Mock (Simulated Data)",
        _                      => plugin,
    }
}

fn load_settings() -> (AppSettings, bool) {
    let path = dirs::config_dir()
        .map(|p| p.join("simtrace").join("settings.toml"))
        .or_else(|| dirs::home_dir().map(|p| p.join(".simtrace").join("settings.toml")));
    if let Some(p) = path {
        if let Ok(s) = AppSettings::load(&p) { return (s, true); }
    }
    (AppSettings::default(), false)
}

fn save_settings(settings: &AppSettings) -> Result<(), anyhow::Error> {
    let path = dirs::config_dir()
        .map(|p| p.join("simtrace").join("settings.toml"))
        .or_else(|| dirs::home_dir().map(|p| p.join(".simtrace").join("settings.toml")))
        .ok_or_else(|| anyhow::anyhow!("No config dir"))?;
    if let Some(p) = path.parent() { std::fs::create_dir_all(p)?; }
    settings.save(&path)
}

// ── Stadium shape ─────────────────────────────────────────────────────────────
// Flat-left rectangle with a semicircular right cap.
// cap_r ≤ rect.height()/2 keeps the shape convex.
fn stadium_path(rect: egui::Rect, left_r: f32, cap_r: f32) -> Vec<egui::Pos2> {
    use std::f32::consts::{FRAC_PI_2, PI};
    let cap_r  = cap_r.min(rect.height() / 2.0);
    let cx     = rect.max.x - cap_r;   // arc centre x
    let cy     = rect.center().y;       // arc centre y
    let cap_ty = cy - cap_r;            // arc top y
    let cap_by = cy + cap_r;            // arc bottom y

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
