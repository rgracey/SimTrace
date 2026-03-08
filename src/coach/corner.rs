//! Corner detection from lap telemetry.
//!
//! After each completed lap, `CornerDetector::detect` analyses the speed
//! profile to find corners. On subsequent laps, `CornerDetector::refine`
//! updates each corner's positions with an EWMA so the map converges.

use serde::{Deserialize, Serialize};

use super::lap::LapSample;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CornerDirection {
    Left,
    Right,
}

/// A corner as it appears in a saved track map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedCorner {
    /// 1-based index, ordered by track position.
    pub id: u8,
    pub direction: CornerDirection,
    /// Track position where braking typically starts.
    pub brake_point: f32,
    /// Track position of the geometric turn-in.
    pub turn_in: f32,
    /// Track position of the apex (speed minimum).
    pub apex: f32,
    /// Track position where meaningful throttle begins on exit.
    pub exit: f32,
    /// How many laps have contributed to this corner's positions (0 = provisional).
    pub confidence: u8,
}

pub struct CornerDetector;

impl CornerDetector {
    /// Detect corners from a completed lap's samples.
    ///
    /// Returns an empty vec if there are too few samples or no clear corners
    /// are found. Corners are sorted by apex track position.
    pub fn detect(samples: &[LapSample]) -> Vec<DetectedCorner> {
        if samples.len() < 60 {
            return vec![];
        }

        // Sort a copy by track position so the speed profile is monotonic.
        let mut sorted = samples.to_vec();
        sorted.sort_by(|a, b| {
            a.track_pos
                .partial_cmp(&b.track_pos)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let speeds: Vec<f32> = sorted.iter().map(|s| s.speed_kph).collect();
        let smoothed = rolling_avg(&speeds, 9);

        let lap_avg_speed: f32 = speeds.iter().sum::<f32>() / speeds.len() as f32;
        let minima = local_minima(&smoothed, 20);

        let mut corners: Vec<DetectedCorner> = Vec::new();

        for idx in minima {
            let apex_speed = smoothed[idx];

            // Must be below lap-average speed (filters slow-down for marshals, etc.).
            if apex_speed >= lap_avg_speed {
                continue;
            }

            // Find the peak speed on either side within a ±25% window.
            let window = (sorted.len() / 4).max(20).min(sorted.len() - 1);
            let lo = idx.saturating_sub(window);
            let hi = (idx + window).min(smoothed.len() - 1);
            let surrounding_max = smoothed[lo..=hi]
                .iter()
                .cloned()
                .fold(f32::NEG_INFINITY, f32::max);

            // Require at least a 10% speed drop from the surrounding peak.
            if surrounding_max - apex_speed < surrounding_max * 0.10 {
                continue;
            }

            // Require meaningful steering at the apex.
            if sorted[idx].steering_angle.abs() < 5.0 {
                continue;
            }

            let direction = if sorted[idx].steering_angle >= 0.0 {
                CornerDirection::Right
            } else {
                CornerDirection::Left
            };

            let brake_point = find_brake_point(&sorted, idx);
            let turn_in = find_turn_in(&sorted, idx);
            let exit = find_exit_point(&sorted, idx);

            corners.push(DetectedCorner {
                id: 0, // assigned after deduplication
                direction,
                brake_point,
                turn_in,
                apex: sorted[idx].track_pos,
                exit,
                confidence: 1,
            });
        }

        // Merge corners whose apices are within 3% of each other.
        corners = merge_nearby(corners, 0.03);

        // Assign stable IDs ordered by apex position.
        for (i, c) in corners.iter_mut().enumerate() {
            c.id = (i + 1) as u8;
        }

        corners
    }

    /// Refine an existing corner's positions from a new lap using EWMA (α = 0.25).
    ///
    /// The new observation has 25% weight; the existing estimate retains 75%.
    /// Confidence is incremented up to a ceiling of 20.
    pub fn refine(corner: &mut DetectedCorner, new_samples: &[LapSample]) {
        // Find samples within 4% of the known apex.
        let nearby: Vec<&LapSample> = new_samples
            .iter()
            .filter(|s| (s.track_pos - corner.apex).abs() < 0.04)
            .collect();

        if nearby.is_empty() {
            return;
        }

        let new_apex = nearby
            .iter()
            .min_by(|a, b| {
                a.speed_kph
                    .partial_cmp(&b.speed_kph)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|s| s.track_pos);

        let Some(new_apex_pos) = new_apex else {
            return;
        };

        // Find the index for this apex in the sorted new samples.
        let mut sorted = new_samples.to_vec();
        sorted.sort_by(|a, b| {
            a.track_pos
                .partial_cmp(&b.track_pos)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let Some(idx) = sorted
            .iter()
            .position(|s| (s.track_pos - new_apex_pos).abs() < 0.002)
        else {
            return;
        };

        const ALPHA: f32 = 0.25;
        let new_brake = find_brake_point(&sorted, idx);
        let new_exit = find_exit_point(&sorted, idx);

        corner.apex = lerp(corner.apex, new_apex_pos, ALPHA);
        corner.brake_point = lerp(corner.brake_point, new_brake, ALPHA);
        corner.exit = lerp(corner.exit, new_exit, ALPHA);
        corner.confidence = (corner.confidence + 1).min(20);
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn rolling_avg(values: &[f32], window: usize) -> Vec<f32> {
    let half = window / 2;
    values
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let lo = i.saturating_sub(half);
            let hi = (i + half + 1).min(values.len());
            values[lo..hi].iter().sum::<f32>() / (hi - lo) as f32
        })
        .collect()
}

/// Returns indices of local minima with at least `min_gap` samples between them.
fn local_minima(values: &[f32], min_gap: usize) -> Vec<usize> {
    let mut result: Vec<usize> = Vec::new();

    for i in 1..values.len().saturating_sub(1) {
        if values[i] < values[i - 1] && values[i] <= values[i + 1] {
            if let Some(&last) = result.last() {
                if i - last < min_gap {
                    // Replace if this minimum is lower.
                    if values[i] < values[last] {
                        *result.last_mut().unwrap() = i;
                    }
                    continue;
                }
            }
            result.push(i);
        }
    }

    result
}

fn find_brake_point(samples: &[LapSample], apex_idx: usize) -> f32 {
    for i in (0..apex_idx).rev() {
        if samples[i].brake > 0.05 {
            return samples[i].track_pos;
        }
    }
    samples[0].track_pos
}

fn find_turn_in(samples: &[LapSample], apex_idx: usize) -> f32 {
    for i in (0..apex_idx).rev() {
        if samples[i].steering_angle.abs() < 5.0 {
            // The sample after this is where steering committed.
            return samples[(i + 1).min(apex_idx)].track_pos;
        }
    }
    samples[0].track_pos
}

fn find_exit_point(samples: &[LapSample], apex_idx: usize) -> f32 {
    for i in (apex_idx + 1)..samples.len().saturating_sub(1) {
        let rising = samples[i].speed_kph > samples[i - 1].speed_kph;
        if samples[i].throttle > 0.20 && rising {
            return samples[i].track_pos;
        }
    }
    samples.last().map(|s| s.track_pos).unwrap_or(1.0)
}

fn merge_nearby(corners: Vec<DetectedCorner>, min_sep: f32) -> Vec<DetectedCorner> {
    let mut result: Vec<DetectedCorner> = Vec::new();
    for corner in corners {
        if let Some(last) = result.last() {
            if (corner.apex - last.apex).abs() < min_sep {
                // Keep whichever has the earlier brake point (more informative).
                continue;
            }
        }
        result.push(corner);
    }
    result
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
