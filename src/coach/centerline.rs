//! Smoothed world-space centerline for one track layout.
//!
//! Built from driven lap data: each lap's XZ coordinates are resampled to a
//! fixed 2 000-point grid and EWMA-blended into a running average.  Curvature
//! is computed from the resulting path using the Menger three-point formula.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::lap::LapSample;

/// Number of evenly-spaced points in the resampled centerline.
const N: usize = 2000;

/// EWMA weight applied to each new lap's contribution (30 %).
const BLEND_ALPHA: f32 = 0.3;

/// Rolling-average window for curvature smoothing (≈ 1 % of N).
const SMOOTH_WIN: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CenterlinePoint {
    /// Normalised track position 0.0–1.0, uniform spacing.
    pub track_pos: f32,
    pub x: f32,
    pub z: f32,
    /// Signed curvature in m⁻¹.  Positive = left turn; negative = right turn.
    pub curvature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Centerline {
    pub track_name: String,
    pub points: Vec<CenterlinePoint>,
    /// How many laps have been averaged into this centerline.
    pub laps_averaged: u32,
    /// Approximate track length derived from the XZ path (metres).
    pub track_length_m: f32,
}

impl Centerline {
    /// Build a centerline from the first complete lap.
    ///
    /// Returns `None` when fewer than 500 samples carry non-zero XZ (plugin
    /// does not supply world coordinates).
    pub fn from_lap(track_name: &str, samples: &[LapSample]) -> Option<Self> {
        let xz: Vec<&LapSample> = samples
            .iter()
            .filter(|s| s.world_x != 0.0 || s.world_z != 0.0)
            .collect();

        if xz.len() < 500 {
            return None;
        }

        let resampled = resample(&xz);
        let track_length_m = path_length(&resampled);
        let curvature = compute_curvature(&resampled, track_length_m);
        let points = build_points(&resampled, &curvature);

        Some(Self {
            track_name: track_name.to_string(),
            points,
            laps_averaged: 1,
            track_length_m,
        })
    }

    /// Blend a subsequent lap into the running average.
    pub fn blend_lap(&mut self, samples: &[LapSample]) {
        let xz: Vec<&LapSample> = samples
            .iter()
            .filter(|s| s.world_x != 0.0 || s.world_z != 0.0)
            .collect();

        if xz.len() < 500 {
            return;
        }

        let new_pts = resample(&xz);
        for (i, pt) in self.points.iter_mut().enumerate() {
            pt.x = (1.0 - BLEND_ALPHA) * pt.x + BLEND_ALPHA * new_pts[i].0;
            pt.z = (1.0 - BLEND_ALPHA) * pt.z + BLEND_ALPHA * new_pts[i].1;
        }

        // Recompute derived quantities after blending.
        self.track_length_m = path_length_from_points(&self.points);
        let curvature = compute_curvature_from_points(&self.points, self.track_length_m);
        for (pt, k) in self.points.iter_mut().zip(curvature.iter()) {
            pt.curvature = *k;
        }

        self.laps_averaged += 1;
    }

    /// Return the `track_pos` of the centerline point nearest to `(x, z)`.
    #[allow(dead_code)]
    pub fn nearest_track_pos(&self, x: f32, z: f32) -> f32 {
        self.points
            .iter()
            .min_by(|a, b| {
                let da = (a.x - x).powi(2) + (a.z - z).powi(2);
                let db = (b.x - x).powi(2) + (b.z - z).powi(2);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|p| p.track_pos)
            .unwrap_or(0.0)
    }

    pub fn save(&self, dir: &Path, stem: &str) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{stem}_centerline.json"));
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load(dir: &Path, stem: &str) -> Option<Self> {
        let path = dir.join(format!("{stem}_centerline.json"));
        let json = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&json).ok()
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Resample `samples` (sorted by `track_pos`) to exactly N evenly-spaced points.
/// Returns a Vec of `(x, z)` tuples.
fn resample(samples: &[&LapSample]) -> Vec<(f32, f32)> {
    // Ensure sorted by track_pos.
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| {
        a.track_pos
            .partial_cmp(&b.track_pos)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut result = Vec::with_capacity(N);
    for i in 0..N {
        let target = i as f32 / N as f32;
        // Binary search for the surrounding pair.
        let pos = sorted
            .partition_point(|s| s.track_pos <= target)
            .saturating_sub(1);

        if pos + 1 >= sorted.len() {
            // Past the end — use last sample.
            let last = sorted.last().unwrap();
            result.push((last.world_x, last.world_z));
        } else {
            let lo = sorted[pos];
            let hi = sorted[pos + 1];
            let span = hi.track_pos - lo.track_pos;
            let t = if span > 1e-9 {
                (target - lo.track_pos) / span
            } else {
                0.0
            };
            result.push((
                lo.world_x + t * (hi.world_x - lo.world_x),
                lo.world_z + t * (hi.world_z - lo.world_z),
            ));
        }
    }
    result
}

fn path_length(pts: &[(f32, f32)]) -> f32 {
    pts.windows(2)
        .map(|w| {
            let dx = w[1].0 - w[0].0;
            let dz = w[1].1 - w[0].1;
            (dx * dx + dz * dz).sqrt()
        })
        .sum()
}

fn path_length_from_points(pts: &[CenterlinePoint]) -> f32 {
    pts.windows(2)
        .map(|w| {
            let dx = w[1].x - w[0].x;
            let dz = w[1].z - w[0].z;
            (dx * dx + dz * dz).sqrt()
        })
        .sum()
}

/// Compute curvature from resampled (x, z) tuples.
fn compute_curvature(pts: &[(f32, f32)], track_length_m: f32) -> Vec<f32> {
    let raw = menger_curvature(
        pts.len(),
        |i| pts[i].0,
        |i| pts[i].1,
        track_length_m,
    );
    rolling_avg(&raw, SMOOTH_WIN)
}

/// Compute curvature from `CenterlinePoint` slice.
fn compute_curvature_from_points(pts: &[CenterlinePoint], track_length_m: f32) -> Vec<f32> {
    let raw = menger_curvature(
        pts.len(),
        |i| pts[i].x,
        |i| pts[i].z,
        track_length_m,
    );
    rolling_avg(&raw, SMOOTH_WIN)
}

/// Three-point Menger curvature, sign from cross product.
/// Units: m⁻¹.
fn menger_curvature(
    n: usize,
    x: impl Fn(usize) -> f32,
    z: impl Fn(usize) -> f32,
    track_length_m: f32,
) -> Vec<f32> {
    let mut k = vec![0.0f32; n];
    // Scale factor: each grid step corresponds to track_length_m / N metres.
    let step_m = track_length_m / n as f32;

    for i in 1..n - 1 {
        let ax = x(i - 1);
        let az = z(i - 1);
        let bx = x(i);
        let bz = z(i);
        let cx = x(i + 1);
        let cz = z(i + 1);

        let ab = ((bx - ax).powi(2) + (bz - az).powi(2)).sqrt().max(1e-9);
        let bc = ((cx - bx).powi(2) + (cz - bz).powi(2)).sqrt().max(1e-9);
        let ca = ((ax - cx).powi(2) + (az - cz).powi(2)).sqrt().max(1e-9);

        let area = 0.5 * ((bx - ax) * (cz - az) - (cx - ax) * (bz - az)).abs();
        let unsigned = 4.0 * area / (ab * bc * ca);

        // Sign: cross product of (B - A) × (C - B).
        let cross = (bx - ax) * (cz - bz) - (bz - az) * (cx - bx);
        k[i] = if cross >= 0.0 { unsigned } else { -unsigned };
        // Scale to m⁻¹: the formula gives curvature in "per metre of path".
        // Each step is ~step_m metres, so we scale accordingly.
        // Actually, when A/B/C are in world metres, the Menger formula already
        // gives 1/R in m⁻¹ — no additional scaling needed for signed value.
        let _ = step_m; // kept for documentation
    }
    // Fill edges.
    if n > 2 {
        k[0] = k[1];
        k[n - 1] = k[n - 2];
    }
    k
}

fn build_points(pts: &[(f32, f32)], curvature: &[f32]) -> Vec<CenterlinePoint> {
    pts.iter()
        .enumerate()
        .map(|(i, &(x, z))| CenterlinePoint {
            track_pos: i as f32 / N as f32,
            x,
            z,
            curvature: curvature[i],
        })
        .collect()
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
