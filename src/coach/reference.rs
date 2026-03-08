//! Reference lap — per-corner performance data used as a coaching baseline.
//!
//! One JSON file per track × car combination, stored in
//! `<data_dir>/references/<track_stem>/self__<car>.json`.
//!
//! The file format is versioned so the schema can evolve without breaking
//! existing saves. Future "pro" and "community" sources will occupy
//! sibling files in the same directory.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::lap::{LapData, LapSample};
use super::track_map::TrackMap;

const FORMAT_VERSION: u32 = 1;

/// Where a reference lap originated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceSource {
    /// Recorded from the driver using this installation.
    SelfRecorded,
    /// Downloaded from the community pool (future).
    Community,
    /// Downloaded from a verified professional reference (future).
    Pro,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceMeta {
    pub version: u32,
    pub source: ReferenceSource,
    pub car_name: String,
    pub game: String,
    pub lap_time_ms: Option<u32>,
    /// Unix timestamp seconds when this reference was recorded.
    pub recorded_at_unix: u64,
}

/// Measured performance at a single corner during a reference lap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CornerPerf {
    pub corner_id: u8,
    /// Speed at the brake point entry (kph).
    pub entry_speed_kph: f32,
    /// Minimum speed at the apex (kph).
    pub apex_speed_kph: f32,
    /// Speed at the throttle application point on exit (kph).
    pub exit_speed_kph: f32,
    /// Track position where braking began.
    pub brake_point: f32,
    /// Track position where throttle first exceeded 20%.
    pub throttle_point: f32,
    /// Gear at the apex.
    pub gear: i32,
    /// Number of ABS activations through this corner.
    pub abs_activations: u32,
    /// Number of TC activations on exit.
    pub tc_activations: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceLap {
    pub meta: ReferenceMeta,
    pub corners: Vec<CornerPerf>,
    pub lap_time_ms: Option<u32>,
}

impl ReferenceLap {
    /// Build a reference lap from completed `LapData` and the current `TrackMap`.
    pub fn from_lap(lap: &LapData, map: &TrackMap, game: &str) -> Self {
        let recorded_at_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            meta: ReferenceMeta {
                version: FORMAT_VERSION,
                source: ReferenceSource::SelfRecorded,
                car_name: lap.car_name.clone(),
                game: game.to_string(),
                lap_time_ms: lap.lap_time_ms,
                recorded_at_unix,
            },
            corners: extract_corner_perfs(map, &lap.samples),
            lap_time_ms: lap.lap_time_ms,
        }
    }

    /// Returns the performance record for the given corner ID, if present.
    pub fn corner(&self, id: u8) -> Option<&CornerPerf> {
        self.corners.iter().find(|c| c.corner_id == id)
    }

    /// Is this reference better (faster) than `other`?
    pub fn is_better_than(&self, other: &Self) -> bool {
        match (self.lap_time_ms, other.lap_time_ms) {
            (Some(a), Some(b)) => a < b,
            (Some(_), None) => true,
            _ => false,
        }
    }

    // ── Persistence ──────────────────────────────────────────────────────────

    pub fn save(&self, references_dir: &Path, track_stem: &str) -> anyhow::Result<()> {
        let dir = references_dir.join(track_stem);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(self.filename());
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load the driver's own reference for a given track stem and car name.
    pub fn load_self(references_dir: &Path, track_stem: &str, car_name: &str) -> Option<Self> {
        let filename = self_filename(car_name);
        let path = references_dir.join(track_stem).join(filename);
        let json = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&json).ok()
    }

    fn filename(&self) -> String {
        match self.meta.source {
            ReferenceSource::SelfRecorded => self_filename(&self.meta.car_name),
            ReferenceSource::Community => "community_best.json".to_string(),
            ReferenceSource::Pro => "pro.json".to_string(),
        }
    }
}

fn self_filename(car_name: &str) -> String {
    let safe: String = car_name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    format!("self__{}.json", safe)
}

fn extract_corner_perfs(map: &TrackMap, samples: &[LapSample]) -> Vec<CornerPerf> {
    let mut perfs = Vec::new();

    for corner in &map.corners {
        let in_corner: Vec<&LapSample> = samples
            .iter()
            .filter(|s| s.track_pos >= corner.brake_point && s.track_pos <= corner.exit)
            .collect();

        if in_corner.is_empty() {
            continue;
        }

        let entry_speed = in_corner.first().map(|s| s.speed_kph).unwrap_or(0.0);

        let apex_speed = in_corner
            .iter()
            .map(|s| s.speed_kph)
            .fold(f32::INFINITY, f32::min);

        let exit_speed = in_corner.last().map(|s| s.speed_kph).unwrap_or(0.0);

        let brake_point = in_corner
            .iter()
            .find(|s| s.brake > 0.05)
            .map(|s| s.track_pos)
            .unwrap_or(corner.brake_point);

        let throttle_point = in_corner
            .iter()
            .find(|s| s.track_pos > corner.apex && s.throttle > 0.20)
            .map(|s| s.track_pos)
            .unwrap_or(corner.exit);

        let gear = in_corner
            .iter()
            .min_by(|a, b| {
                a.speed_kph
                    .partial_cmp(&b.speed_kph)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|s| s.gear)
            .unwrap_or(2);

        // Count leading-edge transitions (false → true).
        let abs_activations = in_corner
            .windows(2)
            .filter(|w| !w[0].abs_active && w[1].abs_active)
            .count() as u32;

        let tc_activations = in_corner
            .windows(2)
            .filter(|w| !w[0].tc_active && w[1].tc_active)
            .count() as u32;

        perfs.push(CornerPerf {
            corner_id: corner.id,
            entry_speed_kph: entry_speed,
            apex_speed_kph: apex_speed,
            exit_speed_kph: exit_speed,
            brake_point,
            throttle_point,
            gear,
            abs_activations,
            tc_activations,
        });
    }

    perfs
}
