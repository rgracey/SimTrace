//! Scrolling trace graph for brake/throttle visualization

use egui::{Pos2, Rect, Response, Stroke, Ui, Vec2};

use crate::config::{AppSettings, ColorScheme, GraphSettings};
use crate::core::{TelemetryBuffer, TelemetryPoint};

/// Trace graph renderer
pub struct TraceGraph<'a> {
    buffer: Option<&'a TelemetryBuffer>,
    settings: &'a GraphSettings,
    colors: &'a ColorScheme,
    opacity: f32,
}

impl<'a> TraceGraph<'a> {
    /// Create a new trace graph renderer with buffer
    pub fn new(
        buffer: &'a TelemetryBuffer,
        settings: &'a GraphSettings,
        colors: &'a ColorScheme,
        opacity: f32,
    ) -> Self {
        Self {
            buffer: Some(buffer),
            settings,
            colors,
            opacity,
        }
    }

    /// Create a simple trace graph renderer (with optional buffer)
    pub fn new_simple(
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

        // Draw traces if we have data
        if let Some(buffer) = self.buffer {
            let points = buffer.get_points();
            if !points.is_empty() {
                self.draw_brake_trace(&painter, rect, &points);
                self.draw_throttle_trace(&painter, rect, &points);
            }
        }

        // Draw legend
        if self.settings.show_legend {
            self.draw_legend(&painter, rect);
        }

        response
    }

    /// Render a simple trace graph (overlay version without buffer access)
    pub fn show_simple(&self, ui: &mut Ui, size: Vec2) -> Response {
        // Use empty Sense to prevent hover from triggering repaints
        let (rect, response) = ui.allocate_exact_size(
            size,
            egui::Sense {
                click: false,
                drag: false,
                focusable: false,
            },
        );
        let painter = ui.painter().with_clip_rect(rect);

        // Draw semi-transparent background based on opacity setting
        let bg_color = self.apply_opacity(&self.colors.background);
        painter.rect_filled(rect, 0.0, bg_color);

        // Draw grid with opacity (optional)
        if self.settings.show_grid {
            self.draw_grid(&painter, rect);
        }

        // Draw traces if we have a buffer
        if let Some(buffer) = self.buffer {
            let points = buffer.get_points();
            if !points.is_empty() {
                self.draw_brake_trace(&painter, rect, &points);
                self.draw_throttle_trace(&painter, rect, &points);
            }
        }

        // Draw legend (optional)
        if self.settings.show_legend {
            self.draw_legend(&painter, rect);
        }

        response
    }

    fn draw_grid(&self, painter: &egui::Painter, rect: Rect) {
        let grid_color = self.apply_opacity(&self.colors.grid);
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
        let color = self.apply_opacity(&self.colors.throttle);
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
                self.apply_opacity(&self.colors.abs_active)
            } else {
                self.apply_opacity(&self.colors.brake)
            };

            let stroke = Stroke::new(self.settings.line_width, color);
            painter.add(egui::Shape::line(segment_points, stroke));
        }
    }

    fn draw_legend(&self, painter: &egui::Painter, rect: Rect) {
        let text_color = self.apply_opacity(&self.colors.text);
        let legend_bg = self
            .apply_opacity(&self.colors.background)
            .linear_multiply(0.8);

        let legend_rect =
            Rect::from_min_size(rect.min + Vec2::new(10.0, 10.0), Vec2::new(120.0, 70.0));

        painter.rect_filled(legend_rect, 4.0, legend_bg);

        // Throttle
        let throttle_color = self.apply_opacity(&self.colors.throttle);
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
        let brake_color = self.apply_opacity(&self.colors.brake);
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
        let abs_color = self.apply_opacity(&self.colors.abs_active);
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
        // Invert Y (0 = bottom, 1 = top) with vertical padding so 100% doesn't clip
        let pad = rect.height() * 0.03;
        rect.max.y - pad - ((rect.height() - 1.15 * pad) * value)
    }

    /// Apply opacity to a color
    fn apply_opacity(&self, color_hex: &str) -> egui::Color32 {
        let base_color = AppSettings::parse_color(color_hex);
        // Multiply existing alpha by the opacity factor
        let [r, g, b, a] = base_color.to_array();
        let new_alpha = ((a as f32) * self.opacity) as u8;
        egui::Color32::from_rgba_unmultiplied(r, g, b, new_alpha)
    }
}
