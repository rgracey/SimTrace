//! Steering wheel — ring + sweep arc

use egui::{Color32, Painter, Pos2, Shape, Stroke};
use std::f32::consts::{FRAC_PI_2, TAU};

pub struct SteeringWheel;

impl SteeringWheel {
    /// Draw the steering wheel widget.
    ///
    /// `angle_deg`  — current steering angle in degrees (negative = left).
    /// `max_angle`  — half-lock in degrees (e.g. 450 for a 900° wheel).
    ///                The visual sweep is normalised against this so full lock
    ///                always places the dot at ±270° (3/9 o'clock position).
    pub fn draw(
        painter: &Painter,
        center: Pos2,
        radius: f32,
        angle_deg: f32,
        max_angle: f32,
        opacity: f32,
    ) {
        let a = (opacity * 255.0) as u8;
        let thickness = (radius * 0.28).max(5.0);

        // Background ring — drawn as a polyline (same tessellator as the sweep arc below,
        // guaranteeing pixel-perfect alignment between the ring and sweep).
        const RING_STEPS: usize = 120;
        let mut ring: Vec<Pos2> = (0..RING_STEPS)
            .map(|i| {
                let angle = (i as f32 / RING_STEPS as f32) * TAU;
                Pos2::new(
                    center.x + radius * angle.cos(),
                    center.y + radius * angle.sin(),
                )
            })
            .collect();
        ring.push(ring[0]); // close the loop
        painter.add(Shape::line(
            ring,
            Stroke::new(thickness, Color32::from_rgba_unmultiplied(30, 30, 30, a)),
        ));

        // Normalise against max_angle and map to ±270° visual sweep so that full
        // lock always reaches the 3/9 o'clock position.
        let max_angle = max_angle.max(1.0);
        let sweep_deg = (angle_deg / max_angle).clamp(-1.0, 1.0) * 270.0;
        let start = -FRAC_PI_2; // 12 o'clock

        if sweep_deg.abs() > 0.5 {
            let steps = (sweep_deg.abs() as usize).max(4);
            let arc: Vec<Pos2> = (0..=steps)
                .map(|i| {
                    let angle = start + (i as f32 / steps as f32) * sweep_deg.to_radians();
                    Pos2::new(
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                    )
                })
                .collect();
            painter.add(Shape::line(
                arc,
                Stroke::new(thickness, Color32::from_rgba_unmultiplied(220, 220, 220, a)),
            ));
        }

        // Fixed centre tick at 12 o'clock (marks zero/straight-ahead) —
        // a vertical line spanning the ring stroke width.
        let half = thickness * 0.5;
        painter.line_segment(
            [
                Pos2::new(center.x, center.y - radius - half),
                Pos2::new(center.x, center.y - radius + half),
            ],
            Stroke::new(3.5, Color32::from_rgba_unmultiplied(242, 85, 85, a)),
        );
    }
}
