//! Track map panel — drawn inside a dedicated viewport (separate OS window).

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

use crate::coach::{centerline::CenterlinePoint, corner::DetectedCorner};
use crate::core::lap_store::LapPoint;

pub struct TrackMapPanel<'a> {
    centerline: &'a [CenterlinePoint],
    corners: &'a [DetectedCorner],
    car_track_pos: f32,
    ref_samples: Option<&'a [LapPoint]>,
    laps_averaged: u32,
    has_centerline: bool,
    coach_enabled: bool,
}

impl<'a> TrackMapPanel<'a> {
    pub fn new(
        centerline: &'a [CenterlinePoint],
        corners: &'a [DetectedCorner],
        car_track_pos: f32,
        ref_samples: Option<&'a [LapPoint]>,
        laps_averaged: u32,
        has_centerline: bool,
        coach_enabled: bool,
    ) -> Self {
        Self {
            centerline,
            corners,
            car_track_pos,
            ref_samples,
            laps_averaged,
            has_centerline,
            coach_enabled,
        }
    }

    /// Draw the panel. Returns `true` when the close button is clicked.
    pub fn show(&self, ui: &mut egui::Ui, size: Vec2) -> bool {
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::empty());
        let painter = ui.painter().with_clip_rect(rect);

        // Background
        painter.rect_filled(rect, 0.0, Color32::from_rgb(14, 14, 14));

        if !self.has_centerline {
            self.draw_placeholder(&painter, rect);
        } else {
            self.draw_map(&painter, rect);
        }

        // Close button (top-right, matching phase_plot style)
        let close_center = Pos2::new(rect.max.x - 14.0, rect.min.y + 12.0);
        let close_rect = Rect::from_center_size(close_center, Vec2::splat(20.0));
        let close_resp = ui.interact(close_rect, ui.id().with("close"), egui::Sense::click());
        let cross_alpha = if close_resp.hovered() { 230u8 } else { 100u8 };
        let cross_col = Color32::from_rgba_unmultiplied(210, 210, 210, cross_alpha);
        let arm = 4.5_f32;
        let s = Stroke::new(1.5, cross_col);
        painter.line_segment(
            [
                Pos2::new(close_center.x - arm, close_center.y - arm),
                Pos2::new(close_center.x + arm, close_center.y + arm),
            ],
            s,
        );
        painter.line_segment(
            [
                Pos2::new(close_center.x + arm, close_center.y - arm),
                Pos2::new(close_center.x - arm, close_center.y + arm),
            ],
            s,
        );

        close_resp.clicked()
    }

    fn draw_placeholder(&self, painter: &egui::Painter, rect: Rect) {
        let msg = if !self.coach_enabled {
            "Enable the AI coach to build a track map."
        } else {
            "Complete a lap in ACC to build the track map.\n(World coordinates unavailable for other plugins.)"
        };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            msg,
            egui::FontId::proportional(11.0),
            Color32::from_rgb(100, 100, 100),
        );
    }

    fn draw_map(&self, painter: &egui::Painter, rect: Rect) {
        const PAD: f32 = 28.0;

        // Bounding box of all centerline points.
        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        let mut min_z = f32::MAX;
        let mut max_z = f32::MIN;
        for p in self.centerline {
            min_x = min_x.min(p.x);
            max_x = max_x.max(p.x);
            min_z = min_z.min(p.z);
            max_z = max_z.max(p.z);
        }
        let range_x = (max_x - min_x).max(1.0);
        let range_z = (max_z - min_z).max(1.0);

        let draw_w = rect.width() - 2.0 * PAD;
        let draw_h = rect.height() - 2.0 * PAD;
        let scale = (draw_w / range_x).min(draw_h / range_z);

        // Centre the map in the available area.
        let cx = rect.center().x - (min_x + max_x) * 0.5 * scale;
        let cz = rect.center().y - (min_z + max_z) * 0.5 * scale;

        let to_screen = |x: f32, z: f32| Pos2::new(cx + x * scale, cz + z * scale);

        // ── 1. Centerline ────────────────────────────────────────────────────
        let line_col = Color32::from_rgba_premultiplied(160, 160, 160, 100);
        let pts: Vec<Pos2> = self
            .centerline
            .iter()
            .map(|p| to_screen(p.x, p.z))
            .collect();
        if pts.len() >= 2 {
            painter.add(egui::Shape::line(pts, Stroke::new(1.5, line_col)));
        }

        // ── 2. Braking zones (reference lap) ────────────────────────────────
        let brake_col = Color32::from_rgba_premultiplied(220, 45, 45, 200);
        if let Some(ref_samples) = self.ref_samples {
            let mut seg: Vec<Pos2> = Vec::new();
            for pt in ref_samples {
                if pt.brake > 0.05 {
                    seg.push(self.track_pos_to_screen(pt.track_position, &to_screen));
                } else if seg.len() >= 2 {
                    painter.add(egui::Shape::line(
                        std::mem::take(&mut seg),
                        Stroke::new(3.0, brake_col),
                    ));
                } else {
                    seg.clear();
                }
            }
            if seg.len() >= 2 {
                painter.add(egui::Shape::line(seg, Stroke::new(3.0, brake_col)));
            }
        }

        // ── 3. Corner apices ─────────────────────────────────────────────────
        let apex_col = Color32::from_rgb(255, 200, 50);
        for corner in self.corners {
            let pos = self.track_pos_to_screen(corner.apex, &to_screen);
            painter.circle_filled(pos, 4.0, apex_col);
            painter.text(
                pos + Vec2::new(6.0, -6.0),
                egui::Align2::LEFT_TOP,
                format!("{}", corner.id),
                egui::FontId::proportional(10.0),
                apex_col,
            );
        }

        // ── 4. Car position ──────────────────────────────────────────────────
        let car_pos = self.track_pos_to_screen(self.car_track_pos, &to_screen);
        painter.circle_filled(car_pos, 5.0, Color32::WHITE);
        painter.circle_stroke(
            car_pos,
            5.0,
            Stroke::new(1.5, Color32::from_rgb(60, 200, 80)),
        );

        // ── 5. "Building" label ──────────────────────────────────────────────
        if self.laps_averaged < 3 {
            painter.text(
                rect.min + Vec2::new(8.0, 8.0),
                egui::Align2::LEFT_TOP,
                format!("Building map… {} lap(s)", self.laps_averaged),
                egui::FontId::proportional(10.0),
                Color32::from_rgba_premultiplied(160, 160, 160, 120),
            );
        }
    }

    /// Convert a normalised track position to a screen coordinate via the
    /// nearest centerline point.
    fn track_pos_to_screen(&self, track_pos: f32, to_screen: &impl Fn(f32, f32) -> Pos2) -> Pos2 {
        let n = self.centerline.len();
        if n == 0 {
            return Pos2::ZERO;
        }
        let idx = ((track_pos * n as f32) as usize).min(n - 1);
        to_screen(self.centerline[idx].x, self.centerline[idx].z)
    }
}
