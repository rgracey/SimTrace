//! Lap comparison panel — overlays the current lap against a reference lap,
//! with a delta strip showing cumulative time gained/lost.

use crate::config::ParsedColors;
use crate::core::lap_store::LapPoint;
use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};

pub struct LapComparison<'a> {
    /// The saved reference lap, sorted by track_position.
    reference: Option<&'a Vec<LapPoint>>,
    /// The current lap in progress, in push order.
    current: &'a [LapPoint],
    /// Driver's current track position (0–1), used for the cursor.
    current_track_pos: f32,
    colors: &'a ParsedColors,
    opacity: f32,
}

impl<'a> LapComparison<'a> {
    pub fn new(
        reference: Option<&'a Vec<LapPoint>>,
        current: &'a [LapPoint],
        current_track_pos: f32,
        colors: &'a ParsedColors,
        opacity: f32,
    ) -> Self {
        Self {
            reference,
            current,
            current_track_pos,
            colors,
            opacity,
        }
    }

    /// Returns `true` if the close button was clicked.
    pub fn show(&self, ui: &mut egui::Ui, size: Vec2) -> bool {
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::empty());
        let painter = ui.painter().with_clip_rect(rect);

        // Background + border
        painter.rect_filled(rect, 6.0, self.apply_opacity(self.colors.background));
        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(60, 60, 60, 180)),
            egui::StrokeKind::Inside,
        );

        // Title
        painter.text(
            Pos2::new(rect.min.x + 10.0, rect.min.y + 12.0),
            egui::Align2::LEFT_CENTER,
            "LAP COMPARISON",
            egui::FontId::proportional(10.0),
            Color32::from_rgba_unmultiplied(130, 130, 130, (180.0 * self.opacity) as u8),
        );

        // Layout: main plot on top, delta strip below.
        let pad_top = 24.0;
        let pad_side = 10.0;
        let pad_bottom = 6.0;
        let delta_h = 46.0;
        let delta_gap = 4.0;

        let main_rect = Rect::from_min_max(
            Pos2::new(rect.min.x + pad_side, rect.min.y + pad_top),
            Pos2::new(
                rect.max.x - pad_side,
                rect.max.y - pad_bottom - delta_h - delta_gap,
            ),
        );
        let delta_rect = Rect::from_min_max(
            Pos2::new(rect.min.x + pad_side, rect.max.y - pad_bottom - delta_h),
            Pos2::new(rect.max.x - pad_side, rect.max.y - pad_bottom),
        );

        self.draw_main(&painter, main_rect);
        self.draw_delta(&painter, delta_rect);

        // Current-position cursor through both plots.
        let cursor_color =
            Color32::from_rgba_unmultiplied(255, 255, 255, (110.0 * self.opacity) as u8);
        let cursor = Stroke::new(1.0, cursor_color);
        let cx = main_rect.min.x + self.current_track_pos.clamp(0.0, 1.0) * main_rect.width();
        painter.line_segment(
            [
                Pos2::new(cx, main_rect.min.y),
                Pos2::new(cx, main_rect.max.y),
            ],
            cursor,
        );
        let dcx = delta_rect.min.x + self.current_track_pos.clamp(0.0, 1.0) * delta_rect.width();
        painter.line_segment(
            [
                Pos2::new(dcx, delta_rect.min.y),
                Pos2::new(dcx, delta_rect.max.y),
            ],
            cursor,
        );

        // Close button — same style as PhasePlot.
        let close_center = Pos2::new(rect.max.x - 14.0, rect.min.y + 12.0);
        let close_rect = Rect::from_center_size(close_center, Vec2::splat(20.0));
        let close_resp = ui.interact(close_rect, ui.id().with("close"), egui::Sense::click());
        let cross_alpha = if close_resp.hovered() { 230u8 } else { 100u8 };
        let cross = Stroke::new(
            1.5,
            Color32::from_rgba_unmultiplied(210, 210, 210, cross_alpha),
        );
        let arm = 4.5_f32;
        let (cx, cy) = (close_center.x, close_center.y);
        painter.line_segment(
            [Pos2::new(cx - arm, cy - arm), Pos2::new(cx + arm, cy + arm)],
            cross,
        );
        painter.line_segment(
            [Pos2::new(cx + arm, cy - arm), Pos2::new(cx - arm, cy + arm)],
            cross,
        );

        close_resp.clicked()
    }

    // ── Main plot ─────────────────────────────────────────────────────────────

    fn draw_main(&self, painter: &Painter, rect: Rect) {
        // Grid
        let grid = Stroke::new(
            0.5,
            Color32::from_rgba_unmultiplied(50, 50, 50, (200.0 * self.opacity) as u8),
        );
        for i in 1..4 {
            let x = rect.min.x + rect.width() * i as f32 / 4.0;
            painter.line_segment([Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)], grid);
        }
        for i in 1..4 {
            let y = rect.min.y + rect.height() * i as f32 / 4.0;
            painter.line_segment([Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)], grid);
        }

        // Axes
        let axis = Stroke::new(1.0, self.apply_opacity(self.colors.grid));
        painter.line_segment(
            [
                Pos2::new(rect.min.x, rect.max.y),
                Pos2::new(rect.max.x, rect.max.y),
            ],
            axis,
        );
        painter.line_segment(
            [
                Pos2::new(rect.min.x, rect.min.y),
                Pos2::new(rect.min.x, rect.max.y),
            ],
            axis,
        );

        // X-axis tick labels
        let dim = Color32::from_rgba_unmultiplied(75, 75, 75, (220.0 * self.opacity) as u8);
        let font = egui::FontId::proportional(8.0);
        for (frac, label) in [(0.0_f32, "S/F"), (0.25, "25%"), (0.5, "50%"), (0.75, "75%")] {
            painter.text(
                Pos2::new(rect.min.x + frac * rect.width(), rect.max.y + 3.0),
                egui::Align2::CENTER_TOP,
                label,
                font.clone(),
                dim,
            );
        }

        // Y-axis labels
        for (frac, label) in [(0.0_f32, "100%"), (0.5, "50%"), (1.0, "0")] {
            painter.text(
                Pos2::new(rect.min.x - 3.0, rect.min.y + frac * rect.height()),
                egui::Align2::RIGHT_CENTER,
                label,
                font.clone(),
                dim,
            );
        }

        if self.reference.is_none() && self.current.is_empty() {
            self.draw_no_data(
                painter,
                rect,
                "Complete a lap to enable comparison\nor press Set Reference",
            );
            return;
        }

        // Reference traces (dimmed to ~28% — visible but clearly behind).
        if let Some(ref_lap) = self.reference {
            self.draw_trace(painter, rect, ref_lap, |p| p.brake, self.colors.brake, 0.28);
            self.draw_trace(
                painter,
                rect,
                ref_lap,
                |p| p.throttle,
                self.colors.throttle,
                0.28,
            );
        }

        // Current lap traces (full brightness, drawn on top).
        self.draw_trace(
            painter,
            rect,
            self.current,
            |p| p.brake,
            self.colors.brake,
            1.0,
        );
        self.draw_trace(
            painter,
            rect,
            self.current,
            |p| p.throttle,
            self.colors.throttle,
            1.0,
        );
    }

    fn draw_trace(
        &self,
        painter: &Painter,
        rect: Rect,
        lap: &[LapPoint],
        value_fn: impl Fn(&LapPoint) -> f32,
        color: Color32,
        dim: f32,
    ) {
        if lap.len() < 2 {
            return;
        }
        let [r, g, b, a] = color.to_array();
        let alpha = ((a as f32) * self.opacity * dim) as u8;
        let stroke = Stroke::new(1.5, Color32::from_rgba_unmultiplied(r, g, b, alpha));
        let pts: Vec<Pos2> = lap
            .iter()
            .map(|p| {
                let x = rect.min.x + p.track_position.clamp(0.0, 1.0) * rect.width();
                let y = rect.max.y - value_fn(p).clamp(0.0, 1.0) * rect.height();
                Pos2::new(x, y)
            })
            .collect();
        painter.add(egui::Shape::line(pts, stroke));
    }

    // ── Delta strip ───────────────────────────────────────────────────────────

    fn draw_delta(&self, painter: &Painter, rect: Rect) {
        painter.rect_filled(
            rect,
            2.0,
            Color32::from_rgba_unmultiplied(8, 8, 8, (220.0 * self.opacity) as u8),
        );
        painter.rect_stroke(
            rect,
            2.0,
            Stroke::new(
                0.5,
                Color32::from_rgba_unmultiplied(50, 50, 50, (200.0 * self.opacity) as u8),
            ),
            egui::StrokeKind::Middle,
        );

        let dim = Color32::from_rgba_unmultiplied(70, 70, 70, (200.0 * self.opacity) as u8);
        let font = egui::FontId::proportional(8.0);

        painter.text(
            Pos2::new(rect.min.x + 3.0, rect.min.y + 4.0),
            egui::Align2::LEFT_TOP,
            "Δ",
            font.clone(),
            dim,
        );

        let (Some(reference), false) = (self.reference, self.current.is_empty()) else {
            // No reference or no current data — show hint.
            let hint = if self.reference.is_none() {
                "Set a reference lap to see delta"
            } else {
                ""
            };
            if !hint.is_empty() {
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, hint, font, dim);
            }
            return;
        };

        // Compute (track_pos, delta_seconds) for each current-lap point.
        let deltas: Vec<(f32, f32)> = self
            .current
            .iter()
            .filter_map(|pt| {
                let ref_t = interp_elapsed(reference, pt.track_position)?;
                Some((pt.track_position, (pt.elapsed_ms - ref_t) / 1000.0))
            })
            .collect();

        if deltas.len() < 2 {
            return;
        }

        // Auto-scale, capped at ±10 s so one massive outlier doesn't flatten everything.
        let max_abs = deltas
            .iter()
            .map(|(_, d)| d.abs())
            .fold(0.5_f32, f32::max)
            .min(10.0);

        let y_mid = rect.center().y;

        // Zero line
        painter.line_segment(
            [Pos2::new(rect.min.x, y_mid), Pos2::new(rect.max.x, y_mid)],
            Stroke::new(
                0.5,
                Color32::from_rgba_unmultiplied(80, 80, 80, (200.0 * self.opacity) as u8),
            ),
        );

        // Scale labels (top = slower, bottom = faster relative to ref)
        painter.text(
            Pos2::new(rect.max.x - 2.0, rect.min.y + 2.0),
            egui::Align2::RIGHT_TOP,
            format!("+{:.1}s", max_abs),
            font.clone(),
            dim,
        );
        painter.text(
            Pos2::new(rect.max.x - 2.0, rect.max.y - 2.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("-{:.1}s", max_abs),
            font,
            dim,
        );

        // Delta curve — green when ahead (d < 0), red when behind (d > 0).
        for i in 0..deltas.len() - 1 {
            let (p0, d0) = deltas[i];
            let (p1, d1) = deltas[i + 1];
            let x0 = rect.min.x + p0 * rect.width();
            let x1 = rect.min.x + p1 * rect.width();
            let half_h = rect.height() / 2.0;
            let y0 = (y_mid - d0 / max_abs * half_h).clamp(rect.min.y, rect.max.y);
            let y1 = (y_mid - d1 / max_abs * half_h).clamp(rect.min.y, rect.max.y);
            let color = if d0 <= 0.0 {
                Color32::from_rgba_unmultiplied(55, 200, 80, (230.0 * self.opacity) as u8)
            } else {
                Color32::from_rgba_unmultiplied(220, 55, 55, (230.0 * self.opacity) as u8)
            };
            painter.line_segment(
                [Pos2::new(x0, y0), Pos2::new(x1, y1)],
                Stroke::new(1.5, color),
            );
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn draw_no_data(&self, painter: &Painter, rect: Rect, msg: &str) {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            msg,
            egui::FontId::proportional(10.0),
            Color32::from_rgba_unmultiplied(70, 70, 70, (200.0 * self.opacity) as u8),
        );
    }

    fn apply_opacity(&self, color: Color32) -> Color32 {
        let [r, g, b, a] = color.to_array();
        Color32::from_rgba_unmultiplied(r, g, b, ((a as f32) * self.opacity) as u8)
    }
}

/// Linearly interpolate `elapsed_ms` at `track_pos` within a lap that is
/// sorted by `track_position`.
fn interp_elapsed(lap: &[LapPoint], track_pos: f32) -> Option<f32> {
    if lap.is_empty() {
        return None;
    }
    let i = lap.partition_point(|p| p.track_position < track_pos);
    if i == 0 {
        return Some(lap[0].elapsed_ms);
    }
    if i >= lap.len() {
        return Some(lap[lap.len() - 1].elapsed_ms);
    }
    let p0 = &lap[i - 1];
    let p1 = &lap[i];
    let span = p1.track_position - p0.track_position;
    if span < 1e-6 {
        return Some(p0.elapsed_ms);
    }
    let t = (track_pos - p0.track_position) / span;
    Some(p0.elapsed_ms + t * (p1.elapsed_ms - p0.elapsed_ms))
}
