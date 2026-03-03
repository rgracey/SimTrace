//! Steering wheel visualization

use egui::{Color32, Pos2, Response, Stroke, Ui, Vec2};

use crate::config::{AppSettings, SteeringWheelSettings};

/// Steering wheel renderer
pub struct SteeringWheel<'a> {
    settings: &'a SteeringWheelSettings,
    max_steering_angle: f32,
}

impl<'a> SteeringWheel<'a> {
    /// Create a new steering wheel renderer
    pub fn new(settings: &'a SteeringWheelSettings, max_steering_angle: f32) -> Self {
        Self {
            settings,
            max_steering_angle,
        }
    }

    /// Render the steering wheel
    pub fn show(self, ui: &mut Ui, steering_angle: f32, size: Vec2) -> Response {
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter().with_clip_rect(rect);

        let wheel_color = AppSettings::parse_color(&self.settings.color);
        let center_color = AppSettings::parse_color(&self.settings.center_color);
        let text_color = AppSettings::parse_color(&self.settings.text_color);

        let center = rect.center();
        let radius = rect.width() / 2.0 - 10.0;

        // Calculate rotation angle (convert degrees to radians)
        let rotation = self.calculate_rotation(steering_angle);

        // Draw wheel rim
        painter.circle_filled(center, radius, wheel_color);
        painter.circle_stroke(center, radius, Stroke::new(4.0, center_color));

        // Draw wheel spokes (rotated)
        self.draw_spokes(&painter, center, radius, rotation, wheel_color);

        // Draw center hub
        let hub_radius = radius * 0.3;
        painter.circle_filled(center, hub_radius, center_color);
        painter.circle_stroke(center, hub_radius, Stroke::new(3.0, wheel_color));

        // Draw top marker
        let marker_pos = Pos2::new(center.x, center.y - radius + 5.0);
        painter.circle_filled(marker_pos, 5.0, Color32::WHITE);

        // Draw steering angle text if enabled
        if self.settings.show_angle {
            let angle_text = format!("{}°", steering_angle.round());
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                angle_text,
                egui::FontId::proportional(16.0),
                text_color,
            );
        }

        response
    }

    fn calculate_rotation(&self, steering_angle: f32) -> f32 {
        // Clamp angle to max
        let clamped = steering_angle.clamp(-self.max_steering_angle, self.max_steering_angle);
        // Convert to radians (egui rotates clockwise, steering rotates counterclockwise for left)
        // Left steering (negative) should rotate wheel counterclockwise
        (clamped / self.max_steering_angle) * std::f32::consts::FRAC_PI_2
    }

    fn draw_spokes(
        &self,
        painter: &egui::Painter,
        center: Pos2,
        radius: f32,
        rotation: f32,
        color: Color32,
    ) {
        let spoke_thickness = 8.0;
        let _hub_radius = radius * 0.3;

        // Horizontal spoke (rotated)
        let left_end = self.rotate_point(
            center,
            Pos2::new(center.x - radius + 10.0, center.y),
            rotation,
        );
        let right_end = self.rotate_point(
            center,
            Pos2::new(center.x + radius - 10.0, center.y),
            rotation,
        );
        painter.line_segment([left_end, right_end], Stroke::new(spoke_thickness, color));

        // Vertical spoke (rotated) - only bottom half for typical racing wheel
        let bottom_end = self.rotate_point(
            center,
            Pos2::new(center.x, center.y + radius - 10.0),
            rotation,
        );
        painter.line_segment([center, bottom_end], Stroke::new(spoke_thickness, color));
    }

    fn rotate_point(&self, center: Pos2, point: Pos2, angle: f32) -> Pos2 {
        let dx = point.x - center.x;
        let dy = point.y - center.y;

        let cos = angle.cos();
        let sin = angle.sin();

        Pos2::new(
            center.x + dx * cos - dy * sin,
            center.y + dx * sin + dy * cos,
        )
    }
}
