//! Scrolling trace graph for brake/throttle visualization

use egui::{Pos2, Rect, Response, Stroke, Ui, Vec2};

use crate::config::{ColorScheme, GraphSettings};
use crate::core::{TelemetryBuffer, TelemetryPoint};

/// Trace graph renderer
pub struct TraceGraph<'a> {
    buffer: &'a TelemetryBuffer,
    settings: &'a GraphSettings,
    colors: &'a ColorScheme,
}

impl<'a> TraceGraph<'a> {
    /// Create a new trace graph renderer
    pub fn new(
        buffer: &'a TelemetryBuffer,
        settings: &'a GraphSettings,
        colors: &'a ColorScheme,
    ) -> Self {
        Self {
            buffer,
            settings,
            colors,
        }
    }

    /// Render the trace graph
    pub fn show(self, ui: &mut Ui, size: Vec2) -> Response {
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter().with_clip_rect(rect);

        // Draw background
        let bg_color = AppSettings::parse_color(&self.colors.background);
        painter.rect_filled(rect, 0.0, bg_color);

        // Draw grid
        if self.settings.show_grid {
            self.draw_grid(&painter, rect);
        }

        // Draw traces
        let points = self.buffer.get_points();
        if !points.is_empty() {
            self.draw_throttle_trace(&painter, rect, &points);
            self.draw_brake_trace(&painter, rect, &points);
        }

        // Draw legend
        if self.settings.show_legend {
            self.draw_legend(&painter, rect);
        }

        response
    }

    fn draw_grid(&self, painter: &egui::Painter, rect: Rect) {
        let grid_color = AppSettings::parse_color(&self.colors.grid);
        let stroke = Stroke::new(1.0, grid_color);

        // Horizontal grid lines (0%, 25%, 50%, 75%, 100%)
        for i in 0..=4 {
            let y = rect.min.y + (rect.height() * i as f32 / 4.0);
            painter.line_segment([Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)], stroke);
        }

        // Vertical grid lines (every 2 seconds or based on window)
        let window_secs = self.settings.window_seconds;
        let interval = if window_secs <= 5.0 { 1.0 } else { 2.0 };
        let num_lines = (window_secs / interval) as i32;

        for i in 0..=num_lines {
            let x = rect.min.x + (rect.width() * i as f32 / num_lines as f32);
            painter.line_segment([Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)], stroke);
        }
    }

    fn draw_throttle_trace(&self, painter: &egui::Painter, rect: Rect, points: &[TelemetryPoint]) {
        let color = AppSettings::parse_color(&self.colors.throttle);
        let stroke = Stroke::new(self.settings.line_width, color);

        let line_points: Vec<Pos2> = points
            .iter()
            .enumerate()
            .map(|(i, point)| {
                let x = self.x_position(rect, i, points.len());
                let y = self.y_position(rect, point.telemetry.throttle);
                Pos2::new(x, y)
            })
            .collect();

        if line_points.len() > 1 {
            painter.add(egui::Shape::line(line_points, stroke));
        }
    }

    fn draw_brake_trace(&self, painter: &egui::Painter, rect: Rect, points: &[TelemetryPoint]) {
        // Draw brake trace with ABS color changes
        if points.len() < 2 {
            return;
        }

        // Group consecutive points by ABS state
        let mut segments: Vec<(Vec<Pos2>, bool)> = Vec::new();
        let mut current_segment: Vec<Pos2> = Vec::new();
        let mut current_abs_state: Option<bool> = None;

        for (i, point) in points.iter().enumerate() {
            let x = self.x_position(rect, i, points.len());
            let y = self.y_position(rect, point.telemetry.brake);
            let pos = Pos2::new(x, y);

            if Some(point.abs_active) != current_abs_state {
                // Start new segment
                if !current_segment.is_empty() {
                    segments.push((current_segment, current_abs_state.unwrap_or(false)));
                }
                current_segment = vec![pos];
                current_abs_state = Some(point.abs_active);
            } else {
                current_segment.push(pos);
            }
        }

        // Don't forget the last segment
        if !current_segment.is_empty() {
            segments.push((current_segment, current_abs_state.unwrap_or(false)));
        }

        // Draw each segment with appropriate color
        for (segment_points, abs_active) in segments {
            if segment_points.len() < 2 {
                continue;
            }

            let color = if abs_active {
                AppSettings::parse_color(&self.colors.abs_active)
            } else {
                AppSettings::parse_color(&self.colors.brake)
            };

            let stroke = Stroke::new(self.settings.line_width, color);
            painter.add(egui::Shape::line(segment_points, stroke));
        }
    }

    fn draw_legend(&self, painter: &egui::Painter, rect: Rect) {
        let text_color = AppSettings::parse_color(&self.colors.text);
        let legend_bg = AppSettings::parse_color(&self.colors.background).linear_multiply(0.8);

        let legend_rect =
            Rect::from_min_size(rect.min + Vec2::new(10.0, 10.0), Vec2::new(120.0, 70.0));

        painter.rect_filled(legend_rect, 4.0, legend_bg);

        // Throttle
        let throttle_color = AppSettings::parse_color(&self.colors.throttle);
        painter.line_segment(
            [
                Pos2::new(legend_rect.min.x + 5.0, legend_rect.min.y + 15.0),
                Pos2::new(legend_rect.min.x + 30.0, legend_rect.min.y + 15.0),
            ],
            Stroke::new(2.0, throttle_color),
        );
        painter.text(
            Pos2::new(legend_rect.min.x + 35.0, legend_rect.min.y + 10.0),
            egui::Align2::LEFT_BOTTOM,
            "Throttle",
            egui::FontId::proportional(12.0),
            text_color,
        );

        // Brake
        let brake_color = AppSettings::parse_color(&self.colors.brake);
        painter.line_segment(
            [
                Pos2::new(legend_rect.min.x + 5.0, legend_rect.min.y + 35.0),
                Pos2::new(legend_rect.min.x + 30.0, legend_rect.min.y + 35.0),
            ],
            Stroke::new(2.0, brake_color),
        );
        painter.text(
            Pos2::new(legend_rect.min.x + 35.0, legend_rect.min.y + 30.0),
            egui::Align2::LEFT_BOTTOM,
            "Brake",
            egui::FontId::proportional(12.0),
            text_color,
        );

        // ABS
        let abs_color = AppSettings::parse_color(&self.colors.abs_active);
        painter.line_segment(
            [
                Pos2::new(legend_rect.min.x + 5.0, legend_rect.min.y + 55.0),
                Pos2::new(legend_rect.min.x + 30.0, legend_rect.min.y + 55.0),
            ],
            Stroke::new(2.0, abs_color),
        );
        painter.text(
            Pos2::new(legend_rect.min.x + 35.0, legend_rect.min.y + 50.0),
            egui::Align2::LEFT_BOTTOM,
            "ABS",
            egui::FontId::proportional(12.0),
            text_color,
        );
    }

    fn x_position(&self, rect: Rect, index: usize, total: usize) -> f32 {
        if total <= 1 {
            return rect.max.x;
        }
        rect.min.x + (rect.width() * index as f32 / (total - 1) as f32)
    }

    fn y_position(&self, rect: Rect, value: f32) -> f32 {
        // Invert Y (0 = bottom, 1 = top)
        rect.max.y - (rect.height() * value)
    }
}

// Import AppSettings for color parsing
use crate::config::AppSettings;
