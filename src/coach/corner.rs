//! Corner detection from lap telemetry.
//!
//! `DetectedCorner` describes **track geometry only** — where corners are,
//! their direction, and the geometric apex.  It contains no performance data
//! (brake points, exit throttle, apex speeds).  That information lives in
//! `CornerPerf` inside `ReferenceLap` and changes with every car and session.
//!
//! Two detection strategies:
//!
//! **Centerline-based** (preferred): corners are sustained high-curvature zones
//! derived from the world-space path.  The apex is the point of maximum
//! absolute curvature within the zone — a purely geometric quantity.
//!
//! **Speed-based** (fallback): used when world XZ is unavailable (AMS2, mock).
//! Corners are local speed minima.  The apex is the speed minimum, which is an
//! approximation of the geometric apex.
//!
//! `CornerDetector::refine` converges the apex position toward the speed
//! minimum over subsequent laps (useful for the speed-based fallback) and
//! increments `confidence`.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use super::centerline::Centerline;
use super::lap::LapSample;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CornerDirection {
    Left,
    Right,
}

/// A corner described purely in terms of track geometry.
///
/// None of the fields here depend on how fast a particular car/driver went —
/// they describe where the corner *is*, not how to drive it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedCorner {
    /// 1-based index, ordered by track position.
    pub id: u8,
    pub direction: CornerDirection,
    /// Track position where the curvature zone begins (geometric turn-in).
    pub turn_in: f32,
    /// Track position of the geometric apex — peak curvature for centerline
    /// detection, speed minimum for the speed-based fallback.
    pub apex: f32,
    /// Track position where the curvature zone ends.
    pub zone_exit: f32,
    /// How many laps have confirmed this geometry (0 = provisional).
    pub confidence: u8,
}

pub struct CornerDetector;

impl CornerDetector {
    /// Detect corners from a completed lap.
    ///
    /// Uses the centerline's curvature signal when available, otherwise falls
    /// back to speed-based detection.
    pub fn detect(centerline: Option<&Centerline>, samples: &[LapSample]) -> Vec<DetectedCorner> {
        if samples.len() < 60 {
            return vec![];
        }

        let mut sorted = samples.to_vec();
        sorted.sort_by(|a, b| {
            a.track_pos
                .partial_cmp(&b.track_pos)
                .unwrap_or(Ordering::Equal)
        });

        match centerline {
            Some(cl) => detect_by_centerline(cl, &sorted),
            None => detect_by_speed(&sorted),
        }
    }

    /// Converge the apex position toward the speed minimum from a new lap.
    ///
    /// This is most useful for the speed-based fallback path, where the initial
    /// apex estimate may be noisy.  For centerline-based corners the apex is
    /// already geometric and stable, so refinement has little effect.
    pub fn refine(corner: &mut DetectedCorner, new_samples: &[LapSample]) {
        let nearby: Vec<&LapSample> = new_samples
            .iter()
            .filter(|s| (s.track_pos - corner.apex).abs() < 0.05)
            .collect();

        if let Some(new_apex) = nearby
            .iter()
            .min_by(|a, b| {
                a.speed_kph
                    .partial_cmp(&b.speed_kph)
                    .unwrap_or(Ordering::Equal)
            })
            .map(|s| s.track_pos)
        {
            corner.apex = lerp(corner.apex, new_apex, 0.25);
        }

        corner.confidence = (corner.confidence + 1).min(20);
    }
}

// ── Centerline-based detection ────────────────────────────────────────────────

/// Physical curvature threshold: κ > 0.008 m⁻¹ ≈ radius < 125 m.
const CORNER_THRESHOLD: f32 = 0.008;

fn detect_by_centerline(cl: &Centerline, sorted: &[LapSample]) -> Vec<DetectedCorner> {
    let pts = &cl.points;
    let n = pts.len();

    // Mark above-threshold points.
    let in_corner: Vec<bool> = pts
        .iter()
        .map(|p| p.curvature.abs() > CORNER_THRESHOLD)
        .collect();

    // Extract contiguous zones.
    let mut zones: Vec<(usize, usize)> = Vec::new();
    let mut zone_start: Option<usize> = None;
    for i in 0..n {
        match (zone_start, in_corner[i]) {
            (None, true) => zone_start = Some(i),
            (Some(s), false) => {
                zones.push((s, i - 1));
                zone_start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = zone_start {
        zones.push((s, n - 1));
    }

    // Merge zones whose gap is < 0.5 % of the track (noise within one corner).
    const MERGE_GAP: f32 = 0.005;
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in zones {
        if let Some(last) = merged.last_mut() {
            let gap = pts[s].track_pos - pts[last.1].track_pos;
            if gap < MERGE_GAP {
                last.1 = e;
                continue;
            }
        }
        merged.push((s, e));
    }

    // Drop zones spanning < 0.3 % of track (noise).
    const MIN_ZONE: f32 = 0.003;
    let zones: Vec<_> = merged
        .into_iter()
        .filter(|(s, e)| pts[*e].track_pos - pts[*s].track_pos >= MIN_ZONE)
        .collect();

    let mut corners: Vec<DetectedCorner> = Vec::new();
    for (s, e) in zones {
        let zone_pts = &pts[s..=e];

        // Direction from sign of mean curvature.
        let mean_k: f32 = zone_pts.iter().map(|p| p.curvature).sum::<f32>() / zone_pts.len() as f32;
        let direction = if mean_k >= 0.0 {
            CornerDirection::Left
        } else {
            CornerDirection::Right
        };

        // Apex = point of maximum absolute curvature within the zone.
        // This is a purely geometric quantity — it does not depend on speed.
        let apex_local = zone_pts
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                a.curvature
                    .abs()
                    .partial_cmp(&b.curvature.abs())
                    .unwrap_or(Ordering::Equal)
            })
            .map(|(i, _)| i)
            .unwrap_or(zone_pts.len() / 2);

        let turn_in = pts[s].track_pos;
        let apex = pts[s + apex_local].track_pos;
        let zone_exit = pts[e].track_pos;

        // Verify the sorted lap samples have coverage in this zone before
        // accepting — guards against the first partial lap edge case.
        let has_coverage = sorted
            .iter()
            .any(|sample| sample.track_pos >= turn_in && sample.track_pos <= zone_exit);
        if !has_coverage {
            continue;
        }

        corners.push(DetectedCorner {
            id: 0,
            direction,
            turn_in,
            apex,
            zone_exit,
            confidence: 1,
        });
    }

    assign_ids(corners)
}

// ── Speed-based detection (fallback) ─────────────────────────────────────────

fn detect_by_speed(sorted: &[LapSample]) -> Vec<DetectedCorner> {
    let speeds: Vec<f32> = sorted.iter().map(|s| s.speed_kph).collect();
    let smoothed = rolling_avg(&speeds, 9);

    let lap_avg_speed: f32 = speeds.iter().sum::<f32>() / speeds.len() as f32;
    let min_gap = (sorted.len() / 80).max(5);
    let minima = local_minima(&smoothed, min_gap);

    let approach_window = (sorted.len() * 15 / 100).max(20);

    let mut corners: Vec<DetectedCorner> = Vec::new();
    for idx in minima {
        let apex_speed = smoothed[idx];

        if apex_speed >= lap_avg_speed {
            continue;
        }

        let lo = idx.saturating_sub(approach_window);
        let approach_max = smoothed[lo..idx]
            .iter()
            .cloned()
            .fold(f32::NEG_INFINITY, f32::max);

        const MIN_SPEED_DROP_KPH: f32 = 20.0;
        if approach_max - apex_speed < MIN_SPEED_DROP_KPH {
            continue;
        }

        if sorted[idx].steering_angle.abs() < 2.0 {
            continue;
        }

        let direction = if sorted[idx].steering_angle >= 0.0 {
            CornerDirection::Right
        } else {
            CornerDirection::Left
        };

        let turn_in = find_turn_in(sorted, idx);
        let zone_exit = find_zone_exit(sorted, idx);

        corners.push(DetectedCorner {
            id: 0,
            direction,
            turn_in,
            apex: sorted[idx].track_pos,
            zone_exit,
            confidence: 1,
        });
    }

    corners = merge_nearby(corners, 0.015);
    assign_ids(corners)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn assign_ids(mut corners: Vec<DetectedCorner>) -> Vec<DetectedCorner> {
    for (i, c) in corners.iter_mut().enumerate() {
        c.id = (i + 1) as u8;
    }
    corners
}

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

fn local_minima(values: &[f32], min_gap: usize) -> Vec<usize> {
    let mut result: Vec<usize> = Vec::new();
    for i in 1..values.len().saturating_sub(1) {
        if values[i] < values[i - 1] && values[i] <= values[i + 1] {
            if let Some(&last) = result.last() {
                if i - last < min_gap {
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

fn find_turn_in(samples: &[LapSample], apex_idx: usize) -> f32 {
    for i in (0..apex_idx).rev() {
        if samples[i].steering_angle.abs() < 5.0 {
            return samples[(i + 1).min(apex_idx)].track_pos;
        }
    }
    samples[0].track_pos
}

/// Estimate the end of the corner zone: where speed has recovered and
/// throttle is meaningfully applied.  Used only for the speed-based fallback.
fn find_zone_exit(samples: &[LapSample], apex_idx: usize) -> f32 {
    for i in (apex_idx + 1)..samples.len().saturating_sub(1) {
        if samples[i].throttle > 0.20 && samples[i].speed_kph > samples[i - 1].speed_kph {
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
