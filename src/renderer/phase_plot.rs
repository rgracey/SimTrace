//! Brake vs steering phase plot — shows trail braking shape as an X-Y scatter.

use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};
use std::time::Duration;

use crate::config::{GraphSettings, ParsedColors};
use crate::core::TelemetryBuffer;

pub struct PhasePlot<'a> {
    buffer: Option<&'a TelemetryBuffer>,
    settings: &'a GraphSettings,
    colors: &'a ParsedColors,
    opacity: f32,
    max_steering_angle: f32,
}

impl<'a> PhasePlot<'a> {
    pub fn new(
        buffer: Option<&'a TelemetryBuffer>,
        settings: &'a GraphSettings,
        colors: &'a ParsedColors,
        opacity: f32,
        max_steering_angle: f32,
    ) -> Self {
        Self {
            buffer,
            settings,
            colors,
            opacity,
            max_steering_angle,
        }
    }

    /// Returns `true` if the close button was clicked.
    pub fn show(&self, ui: &mut egui::Ui, size: Vec2) -> bool {
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::empty());
        let painter = ui.painter().with_clip_rect(rect);
        self.draw(&painter, rect);

        // Close button — handled here so we can tint on hover.
        let close_center = Pos2::new(rect.max.x - 14.0, rect.min.y + 12.0);
        let close_rect = Rect::from_center_size(close_center, Vec2::splat(20.0));
        let close_resp = ui.interact(close_rect, ui.id().with("close"), egui::Sense::click());

        let alpha = if close_resp.hovered() { 230u8 } else { 100u8 };
        let cross_color = Color32::from_rgba_unmultiplied(210, 210, 210, alpha);
        let arm = 4.5_f32;
        let cx = close_center.x;
        let cy = close_center.y;
        let s = Stroke::new(1.5, cross_color);
        painter.line_segment(
            [Pos2::new(cx - arm, cy - arm), Pos2::new(cx + arm, cy + arm)],
            s,
        );
        painter.line_segment(
            [Pos2::new(cx + arm, cy - arm), Pos2::new(cx - arm, cy + arm)],
            s,
        );

        close_resp.clicked()
    }

    fn draw(&self, painter: &Painter, rect: Rect) {
        let bg = self.apply_opacity(self.colors.background);

        // Rounded background + subtle border.
        painter.rect_filled(rect, 6.0, bg);
        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(60, 60, 60, 180)),
            egui::StrokeKind::Inside,
        );

        let pad_top = 24.0;
        let pad_left = 34.0;
        let pad_bottom = 28.0;
        let pad_right = 10.0;

        let plot = Rect::from_min_max(
            Pos2::new(rect.min.x + pad_left, rect.min.y + pad_top),
            Pos2::new(rect.max.x - pad_right, rect.max.y - pad_bottom),
        );

        // Title.
        let title_color =
            Color32::from_rgba_unmultiplied(130, 130, 130, (180.0 * self.opacity) as u8);
        painter.text(
            Pos2::new(rect.min.x + 10.0, rect.min.y + 12.0),
            egui::Align2::LEFT_CENTER,
            "BRAKE × STEER",
            egui::FontId::proportional(10.0),
            title_color,
        );

        // Grid lines (4×4).
        let grid_stroke = Stroke::new(
            0.5,
            Color32::from_rgba_unmultiplied(60, 60, 60, (200.0 * self.opacity) as u8),
        );
        for i in 1..4 {
            let f = i as f32 / 4.0;
            let y = plot.min.y + plot.height() * (1.0 - f);
            painter.line_segment(
                [Pos2::new(plot.min.x, y), Pos2::new(plot.max.x, y)],
                grid_stroke,
            );
            let x = plot.min.x + plot.width() * f;
            painter.line_segment(
                [Pos2::new(x, plot.min.y), Pos2::new(x, plot.max.y)],
                grid_stroke,
            );
        }

        // Axes.
        let axis_color = self.apply_opacity(self.colors.grid);
        let axis_stroke = Stroke::new(1.0, axis_color);
        painter.line_segment(
            [
                Pos2::new(plot.min.x, plot.max.y),
                Pos2::new(plot.max.x, plot.max.y),
            ],
            axis_stroke,
        );
        painter.line_segment(
            [
                Pos2::new(plot.min.x, plot.min.y),
                Pos2::new(plot.min.x, plot.max.y),
            ],
            axis_stroke,
        );

        // Tick labels.
        let dim = Color32::from_rgba_unmultiplied(100, 100, 100, (220.0 * self.opacity) as u8);
        let font_tick = egui::FontId::proportional(9.0);

        for (brake_val, label) in &[(0.0_f32, "0"), (0.5, "50"), (1.0, "100%")] {
            let y = plot.min.y + (1.0 - brake_val) * plot.height();
            painter.text(
                Pos2::new(plot.min.x - 4.0, y),
                egui::Align2::RIGHT_CENTER,
                *label,
                font_tick.clone(),
                dim,
            );
        }
        for (steer_val, label) in &[(0.0_f32, "0"), (0.5, "50"), (1.0, "100%")] {
            let x = plot.min.x + steer_val * plot.width();
            painter.text(
                Pos2::new(x, plot.max.y + 3.0),
                egui::Align2::CENTER_TOP,
                *label,
                font_tick.clone(),
                dim,
            );
        }

        // Axis labels.
        let text_color = self.apply_opacity(self.colors.text);
        let font_label = egui::FontId::proportional(10.0);
        painter.text(
            Pos2::new(plot.center().x, rect.max.y - 3.0),
            egui::Align2::CENTER_BOTTOM,
            "Steering",
            font_label.clone(),
            text_color,
        );
        painter.text(
            Pos2::new(rect.min.x + 2.0, plot.center().y),
            egui::Align2::LEFT_CENTER,
            "Brake",
            font_label,
            text_color,
        );

        // Data.
        let Some(buffer) = self.buffer else {
            self.draw_no_data(painter, plot);
            return;
        };
        let now = std::time::Instant::now();
        let window_dur = Duration::from_secs_f64(self.settings.window_seconds);
        let points: Vec<_> = buffer
            .get_points()
            .into_iter()
            .filter(|p| now.duration_since(p.captured_at) <= window_dur)
            .collect();

        let steering_threshold = self.settings.trail_brake_threshold * self.max_steering_angle;
        let max_angle = self.max_steering_angle.max(1.0);

        // How long (seconds) a braking trail takes to fully fade after the
        // driver releases the pedal. Independent of window_seconds so the
        // phase plot always feels responsive.
        const FADE_SECS: f32 = 4.0;

        // Build a flat list of (screen pos, base colour, age-based freshness)
        // for braking points only. Non-braking points are skipped to avoid
        // drawing a red line along the bottom of the plot.
        let brake_pts: Vec<(Pos2, Color32, f32)> = points
            .iter()
            .filter(|p| p.telemetry.brake > 0.01)
            .map(|point| {
                let brake = point.telemetry.brake.clamp(0.0, 1.0);
                let steer_norm = (point.telemetry.steering_angle.abs() / max_angle).clamp(0.0, 1.0);
                let px = Pos2::new(
                    plot.min.x + steer_norm * plot.width(),
                    plot.min.y + (1.0 - brake) * plot.height(),
                );
                let is_turning = point.telemetry.steering_angle.abs() > steering_threshold;
                let base = match (point.abs_active, is_turning) {
                    (true, true) if self.settings.show_abs_cornering => self.colors.abs_cornering,
                    (true, _) => self.colors.abs_active,
                    (false, true) if self.settings.show_trail_brake => self.colors.trail_brake,
                    _ => self.colors.brake,
                };
                let age = now.duration_since(point.captured_at).as_secs_f32();
                let freshness = (1.0 - age / FADE_SECS).clamp(0.0, 1.0);
                (px, base, freshness)
            })
            .collect();

        let n = brake_pts.len();
        if n < 2 {
            self.draw_no_data(painter, plot);
            return;
        }

        // Draw each consecutive pair with alpha = freshness^1.5 so the tail
        // fades smoothly to nothing. After brake release the points age out
        // over FADE_SECS, giving a natural dissolve rather than a hard cut.
        let lw = self.settings.line_width;
        for i in 0..n - 1 {
            let (p0, color, freshness) = brake_pts[i];
            let (p1, _, _) = brake_pts[i + 1];
            let fade = freshness.powf(1.5);
            let [r, g, b, a] = color.to_array();
            let alpha = ((a as f32) * self.opacity * fade) as u8;
            painter.line_segment(
                [p0, p1],
                Stroke::new(lw, Color32::from_rgba_unmultiplied(r, g, b, alpha)),
            );
        }
    }

    fn draw_no_data(&self, painter: &Painter, plot: Rect) {
        let color = Color32::from_rgba_unmultiplied(70, 70, 70, (200.0 * self.opacity) as u8);
        painter.text(
            plot.center(),
            egui::Align2::CENTER_CENTER,
            "No data",
            egui::FontId::proportional(11.0),
            color,
        );
    }

    fn apply_opacity(&self, color: Color32) -> Color32 {
        let [r, g, b, a] = color.to_array();
        Color32::from_rgba_unmultiplied(r, g, b, ((a as f32) * self.opacity) as u8)
    }
}
