//! Steering wheel — ring + sweep arc

use egui::{Color32, Painter, Pos2, Shape, Stroke};
use std::f32::consts::FRAC_PI_2;

pub struct SteeringWheel;

impl SteeringWheel {
    pub fn draw(painter: &Painter, center: Pos2, radius: f32, angle_deg: f32, opacity: f32) {
        let a = (opacity * 255.0) as u8;
        let thickness = (radius * 0.28).max(5.0);

        // Background ring (dark track)
        painter.circle_stroke(
            center,
            radius,
            Stroke::new(thickness, Color32::from_rgba_unmultiplied(30, 30, 30, a)),
        );

        // Sweep arc — clamp to ±360° visually, the text handles the rest
        let sweep_deg = angle_deg.clamp(-360.0, 360.0);
        let start = -FRAC_PI_2; // 12 o'clock
        let end = start + sweep_deg.to_radians();

        if sweep_deg.abs() > 0.5 {
            let steps = (sweep_deg.abs() as usize).max(4);
            let arc: Vec<Pos2> = (0..=steps)
                .map(|i| {
                    let angle = start + (i as f32 / steps as f32) * sweep_deg.to_radians();
                    Pos2::new(center.x + radius * angle.cos(), center.y + radius * angle.sin())
                })
                .collect();
            painter.add(Shape::line(
                arc,
                Stroke::new(thickness, Color32::from_rgba_unmultiplied(220, 220, 220, a)),
            ));
        }

        // Dot at tip of sweep
        let tip = Pos2::new(center.x + radius * end.cos(), center.y + radius * end.sin());
        painter.circle_filled(tip, thickness * 0.75, Color32::from_rgba_unmultiplied(255, 255, 255, a));

        // Fixed centre blip at 12 o'clock (marks zero/straight-ahead)
        let top = Pos2::new(center.x, center.y - radius);
        painter.circle_filled(top, thickness * 0.45, Color32::from_rgba_unmultiplied(130, 130, 130, a));

        // If > one full rotation, show the angle in the centre so the driver knows where they are
        if angle_deg.abs() > 360.0 {
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                format!("{:.0}°", angle_deg),
                egui::FontId::proportional((radius * 0.35).max(9.0)),
                Color32::from_rgba_unmultiplied(200, 200, 200, a),
            );
        }
    }
}
