//! Scrolling trace graph visualizing throttle, brake, and clutch over time.
//!
//! Newest data appears on the right; the graph scrolls left as time passes.
//! Point x-coordinates are derived from timestamps so rendering stays correct
//! even when poll intervals are uneven.

use egui::{Pos2, Rect, Response, Stroke, Ui, Vec2};

use crate::config::{AppSettings, ColorScheme, GraphSettings};
use crate::core::{TelemetryBuffer, TelemetryPoint};

/// Renders a scrolling trace of pedal inputs against time.
pub struct TraceGraph<'a> {
    buffer: Option<&'a TelemetryBuffer>,
    settings: &'a GraphSettings,
    colors: &'a ColorScheme,
    opacity: f32,
}

impl<'a> TraceGraph<'a> {
    pub fn new(
        buffer: Option<&'a TelemetryBuffer>,
        settings: &'a GraphSettings,
        colors: &'a ColorScheme,
        opacity: f32,
    ) -> Self {
        Self {
            buffer,
            settings,
            colors,
            opacity,
        }
    }

    pub fn show(&self, ui: &mut Ui, size: Vec2) -> Response {
        let (rect, response) = ui.allocate_exact_size(
            size,
            egui::Sense {
                click: false,
                drag: false,
                focusable: false,
            },
        );
        let painter = ui.painter().with_clip_rect(rect);

        painter.rect_filled(rect, 0.0, self.apply_opacity(&self.colors.background));

        if self.settings.show_grid {
            self.draw_grid(&painter, rect);
        }

        if let Some(buffer) = self.buffer {
            let now = std::time::Instant::now();
            let window_dur = std::time::Duration::from_secs_f64(self.settings.window_seconds);
            let points: Vec<TelemetryPoint> = buffer
                .get_points()
                .into_iter()
                .filter(|p| now.duration_since(p.captured_at) <= window_dur)
                .collect();

            if !points.is_empty() {
                // Draw order: clutch → throttle → brake/ABS (top, always visible)
                self.draw_trace(
                    &painter,
                    rect,
                    &points,
                    now,
                    window_dur,
                    |p| p.telemetry.clutch,
                    &self.colors.clutch,
                );
                self.draw_trace(
                    &painter,
                    rect,
                    &points,
                    now,
                    window_dur,
                    |p| p.telemetry.throttle,
                    &self.colors.throttle,
                );
                self.draw_brake_trace(&painter, rect, &points, now, window_dur);
            }
        }

        if self.settings.show_legend {
            self.draw_legend(&painter, rect);
        }

        response
    }

    fn draw_grid(&self, painter: &egui::Painter, rect: Rect) {
        let stroke = Stroke::new(1.0, self.apply_opacity(&self.colors.grid));

        for i in 0..=4 {
            let y = rect.min.y + rect.height() * i as f32 / 4.0;
            painter.line_segment([Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)], stroke);
        }

        let window_secs = self.settings.window_seconds;
        let interval = if window_secs <= 5.0 { 1.0 } else { 2.0 };
        let num_lines = (window_secs / interval) as i32;
        for i in 0..=num_lines {
            let x = rect.min.x + rect.width() * i as f32 / num_lines as f32;
            painter.line_segment([Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)], stroke);
        }
    }

    /// Draw a single-colour trace for any scalar telemetry value.
    #[allow(clippy::too_many_arguments)]
    fn draw_trace(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        points: &[TelemetryPoint],
        now: std::time::Instant,
        window_dur: std::time::Duration,
        value_fn: impl Fn(&TelemetryPoint) -> f32,
        color_hex: &str,
    ) {
        let stroke = Stroke::new(self.settings.line_width, self.apply_opacity(color_hex));
        let line_points: Vec<Pos2> = points
            .iter()
            .map(|p| {
                Pos2::new(
                    self.x_position(rect, p, now, window_dur),
                    self.y_position(rect, value_fn(p)),
                )
            })
            .collect();
        if line_points.len() > 1 {
            painter.add(egui::Shape::line(line_points, stroke));
        }
    }

    /// Draw the brake trace, colouring ABS-active segments differently.
    fn draw_brake_trace(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        points: &[TelemetryPoint],
        now: std::time::Instant,
        window_dur: std::time::Duration,
    ) {
        if points.len() < 2 {
            return;
        }

        let mut segments: Vec<(Vec<Pos2>, bool)> = Vec::new();
        let mut current_pts: Vec<Pos2> = Vec::new();
        let mut current_abs: Option<bool> = None;

        for point in points {
            let pos = Pos2::new(
                self.x_position(rect, point, now, window_dur),
                self.y_position(rect, point.telemetry.brake),
            );
            if Some(point.abs_active) != current_abs {
                if !current_pts.is_empty() {
                    segments.push((
                        std::mem::take(&mut current_pts),
                        current_abs.unwrap_or(false),
                    ));
                }
                current_pts.push(pos);
                current_abs = Some(point.abs_active);
            } else {
                current_pts.push(pos);
            }
        }
        if !current_pts.is_empty() {
            segments.push((current_pts, current_abs.unwrap_or(false)));
        }

        for (seg_pts, abs_active) in segments {
            if seg_pts.len() < 2 {
                continue;
            }
            let color_hex = if abs_active {
                &self.colors.abs_active
            } else {
                &self.colors.brake
            };
            painter.add(egui::Shape::line(
                seg_pts,
                Stroke::new(self.settings.line_width, self.apply_opacity(color_hex)),
            ));
        }
    }

    fn draw_legend(&self, painter: &egui::Painter, rect: Rect) {
        let text_color = self.apply_opacity(&self.colors.text);
        let bg = self
            .apply_opacity(&self.colors.background)
            .linear_multiply(0.8);
        let legend_rect =
            Rect::from_min_size(rect.min + Vec2::new(10.0, 10.0), Vec2::new(120.0, 90.0));
        painter.rect_filled(legend_rect, 4.0, bg);

        let entries = [
            ("Throttle", self.colors.throttle.as_str()),
            ("Brake", self.colors.brake.as_str()),
            ("ABS", self.colors.abs_active.as_str()),
            ("Clutch", self.colors.clutch.as_str()),
        ];
        for (i, (label, color_hex)) in entries.iter().enumerate() {
            let y = legend_rect.min.y + 15.0 + i as f32 * 20.0;
            painter.line_segment(
                [
                    Pos2::new(legend_rect.min.x + 5.0, y),
                    Pos2::new(legend_rect.min.x + 30.0, y),
                ],
                Stroke::new(2.0, self.apply_opacity(color_hex)),
            );
            painter.text(
                Pos2::new(legend_rect.min.x + 35.0, y - 5.0),
                egui::Align2::LEFT_BOTTOM,
                *label,
                egui::FontId::proportional(12.0),
                text_color,
            );
        }
    }

    /// Maps a data point to its x coordinate based on its timestamp within the
    /// display window. Newest points appear on the right; oldest on the left.
    fn x_position(
        &self,
        rect: Rect,
        point: &TelemetryPoint,
        now: std::time::Instant,
        window_dur: std::time::Duration,
    ) -> f32 {
        let age = now.duration_since(point.captured_at).as_secs_f64();
        let fraction = 1.0 - (age / window_dur.as_secs_f64()) as f32;
        rect.min.x + rect.width() * fraction.clamp(0.0, 1.0)
    }

    fn y_position(&self, rect: Rect, value: f32) -> f32 {
        // Invert Y (0 = bottom, 1 = top) with padding so 100% doesn't clip
        let pad = rect.height() * 0.03;
        rect.max.y - pad - ((rect.height() - 1.15 * pad) * value)
    }

    fn apply_opacity(&self, color_hex: &str) -> egui::Color32 {
        let [r, g, b, a] = AppSettings::parse_color(color_hex).to_array();
        egui::Color32::from_rgba_unmultiplied(r, g, b, ((a as f32) * self.opacity) as u8)
    }
}
