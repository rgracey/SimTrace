//! Corner detection from lap telemetry.
//!
//! Two strategies, selected automatically per lap:
//!
//! **Curvature-based** (preferred): when the plugin supplies a heading/yaw
//! signal, corners are identified as sustained high-curvature zones
//! (Δheading / Δtrack_pos).  This is purely geometric — it is completely
//! independent of how fast the driver took the corner.
//!
//! **Speed-based** (fallback): when heading is unavailable (AMS2, mock) the
//! detector falls back to finding local speed minima with a significant
//! approach-speed drop, the approach that was used before heading support.
//!
//! After the initial detection, `CornerDetector::refine` converges each
//! corner's brake/apex/exit positions toward the true values over subsequent
//! laps using an EWMA (α = 0.25).

use std::cmp::Ordering;

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
    /// Automatically chooses curvature-based or speed-based detection
    /// depending on whether heading data is present.
    pub fn detect(samples: &[LapSample]) -> Vec<DetectedCorner> {
        if samples.len() < 60 {
            return vec![];
        }

        let mut sorted = samples.to_vec();
        sorted.sort_by(|a, b| {
            a.track_pos
                .partial_cmp(&b.track_pos)
                .unwrap_or(Ordering::Equal)
        });

        // Determine whether heading is usable by summing total |rotation|.
        // On a real lap the car completes ≈ 2π radians of turning.  A flat
        // zero heading signal produces essentially 0 total rotation.
        let total_rotation: f32 = sorted
            .windows(2)
            .map(|w| angle_diff_rad(w[1].heading, w[0].heading).abs())
            .sum();

        if total_rotation > 1.0 {
            detect_by_curvature(&sorted)
        } else {
            detect_by_speed(&sorted)
        }
    }

    /// Refine an existing corner's positions from a new lap using EWMA (α = 0.25).
    pub fn refine(corner: &mut DetectedCorner, new_samples: &[LapSample]) {
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
                    .unwrap_or(Ordering::Equal)
            })
            .map(|s| s.track_pos);

        let Some(new_apex_pos) = new_apex else {
            return;
        };

        let mut sorted = new_samples.to_vec();
        sorted.sort_by(|a, b| {
            a.track_pos
                .partial_cmp(&b.track_pos)
                .unwrap_or(Ordering::Equal)
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

// ── Curvature-based detection ─────────────────────────────────────────────────

fn detect_by_curvature(sorted: &[LapSample]) -> Vec<DetectedCorner> {
    let n = sorted.len();

    // --- Step 1: Unwrap heading to a continuous signal ---
    // Raw heading wraps at ±π.  Accumulating angle_diff gives a monotone
    // signal (approx. ±2π per lap) that can be smoothed and differentiated
    // without discontinuity artefacts.
    let mut unwrapped = vec![0.0f32; n];
    unwrapped[0] = sorted[0].heading;
    for i in 1..n {
        unwrapped[i] =
            unwrapped[i - 1] + angle_diff_rad(sorted[i].heading, sorted[i - 1].heading);
    }

    // --- Step 2: Light smoothing of the heading signal ---
    // Just removes per-frame jitter; keep the window small (≈ 0.5 % of lap).
    let h_win = ((n / 200).max(3)).min(15);
    let smooth_h = rolling_avg(&unwrapped, h_win);

    // --- Step 3: Curvature via central difference over a wider window ---
    // Computing dh/dp sample-by-sample amplifies noise enormously because
    // dp ≈ 1/n is tiny.  Widening the base to ≈ 1 % of the lap on each side
    // gives a stable, high-SNR estimate:
    //
    //   curvature[i] = (heading[i + half] − heading[i − half])
    //                / (track_pos[i + half] − track_pos[i − half])
    //
    // Units: radians per lap-fraction (rad / 1.0).
    // Fast sweeper (30° over 5 % lap) → ≈ 10.  Hairpin (180° over 2 %) → ≈ 157.
    // Straight (2° drift over 20 %) → ≈ 0.17.
    let half = ((n / 100).max(5)).min(60); // ≈ 1 % of lap on each side
    let mut curvature = vec![0.0f32; n];
    for i in half..n - half {
        let dh = smooth_h[i + half] - smooth_h[i - half];
        let dp = (sorted[i + half].track_pos - sorted[i - half].track_pos).max(1e-6);
        curvature[i] = dh / dp;
    }
    // Fill edges with nearest valid value.
    for i in 0..half {
        curvature[i] = curvature[half];
    }
    for i in (n - half)..n {
        curvature[i] = curvature[n - half - 1];
    }

    // --- Step 4: Adaptive threshold (mean + 0.5 σ) ---
    // Using mean + 1σ was too conservative: tight hairpins inflate the
    // distribution and push the threshold above moderate/fast corners.
    // 0.5σ reliably includes fast sweepers while keeping straights out.
    let abs_c: Vec<f32> = curvature.iter().map(|c| c.abs()).collect();
    let mean_c = abs_c.iter().sum::<f32>() / n as f32;
    let var_c = abs_c
        .iter()
        .map(|&c| (c - mean_c).powi(2))
        .sum::<f32>()
        / n as f32;
    let std_c = var_c.sqrt();
    let threshold = (mean_c + 0.5 * std_c).max(1.5);

    // Mark samples inside a corner.
    let in_corner: Vec<bool> = abs_c.iter().map(|&c| c > threshold).collect();

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

    // Merge zones whose gap is < 0.5 % of the lap.
    // This only catches a single corner's curvature zone briefly dipping below
    // threshold mid-corner (noise).  Genuine chicane elements are > 0.5 % apart
    // and must stay as separate corners for the coach to address them individually.
    const MERGE_GAP: f32 = 0.005;
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in zones {
        if let Some(last) = merged.last_mut() {
            let gap = sorted[s].track_pos - sorted[last.1].track_pos;
            if gap < MERGE_GAP {
                last.1 = e;
                continue;
            }
        }
        merged.push((s, e));
    }

    // Drop zones that span < 0.5 % of the lap (noise spikes).
    const MIN_ZONE: f32 = 0.005;
    let zones: Vec<_> = merged
        .into_iter()
        .filter(|(s, e)| sorted[*e].track_pos - sorted[*s].track_pos >= MIN_ZONE)
        .collect();

    // Build a DetectedCorner per zone.
    let mut corners: Vec<DetectedCorner> = Vec::new();
    for (s, e) in zones {
        let zone = &sorted[s..=e];

        // Apex = minimum speed within the zone.
        let apex_local = zone
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                a.speed_kph.partial_cmp(&b.speed_kph).unwrap_or(Ordering::Equal)
            })
            .map(|(i, _)| i)
            .unwrap_or((e - s) / 2);
        let apex_global = s + apex_local;

        // Direction from the sign of the mean curvature in the zone.
        // Positive curvature = the heading increases = left turn in most
        // conventions; exact mapping depends on the game's coord system but
        // is consistently applied, which is all the coach needs.
        let mean_curv: f32 =
            curvature[s..=e].iter().sum::<f32>() / (e - s + 1) as f32;
        let direction = if mean_curv >= 0.0 {
            CornerDirection::Left
        } else {
            CornerDirection::Right
        };

        // Turn-in is the start of the high-curvature zone.
        let turn_in = sorted[s].track_pos;
        let brake_point = find_brake_point(&sorted, apex_global);
        let exit = find_exit_point(&sorted, apex_global);

        corners.push(DetectedCorner {
            id: 0,
            direction,
            brake_point,
            turn_in,
            apex: sorted[apex_global].track_pos,
            exit,
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

    // Approach window: look back ~15 % of the lap for the peak speed before
    // each apex.  Backward-only avoids picking up the exit of the next corner.
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

        let brake_point = find_brake_point(sorted, idx);
        let turn_in = find_turn_in(sorted, idx);
        let exit = find_exit_point(sorted, idx);

        corners.push(DetectedCorner {
            id: 0,
            direction,
            brake_point,
            turn_in,
            apex: sorted[idx].track_pos,
            exit,
            confidence: 1,
        });
    }

    // Merge apices within 1.5 % of each other.
    corners = merge_nearby(corners, 0.015);
    assign_ids(corners)
}

// ── Shared helpers ────────────────────────────────────────────────────────────

fn assign_ids(mut corners: Vec<DetectedCorner>) -> Vec<DetectedCorner> {
    for (i, c) in corners.iter_mut().enumerate() {
        c.id = (i + 1) as u8;
    }
    corners
}

/// Signed difference between two angles in radians, normalised to (−π, π].
fn angle_diff_rad(a: f32, b: f32) -> f32 {
    let diff = a - b;
    let pi = std::f32::consts::PI;
    if diff > pi {
        diff - 2.0 * pi
    } else if diff < -pi {
        diff + 2.0 * pi
    } else {
        diff
    }
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
            return samples[(i + 1).min(apex_idx)].track_pos;
        }
    }
    samples[0].track_pos
}

fn find_exit_point(samples: &[LapSample], apex_idx: usize) -> f32 {
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
