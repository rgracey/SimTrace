//! Track map — detected corner geometry for a circuit.
//!
//! One JSON file per track, stored in `<data_dir>/tracks/`.
//! The file name is derived from the track name and length so different
//! layouts of the same venue don't collide.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::corner::DetectedCorner;

const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackMap {
    pub version: u32,
    pub track_name: String,
    /// Metres, 0.0 if the game does not expose track length.
    pub track_length_m: f32,
    pub corners: Vec<DetectedCorner>,
}

impl TrackMap {
    pub fn new(track_name: String, track_length_m: f32, corners: Vec<DetectedCorner>) -> Self {
        Self {
            version: FORMAT_VERSION,
            track_name,
            track_length_m,
            corners,
        }
    }

    /// Stable file stem used for both the track map and the reference lap directory.
    pub fn file_stem(&self) -> String {
        make_safe_name(&self.track_name, self.track_length_m)
    }

    /// Save the map to `<dir>/<stem>.json`, creating the directory if needed.
    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{}.json", self.file_stem()));
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load a track map for the given name and length. Returns `None` if no
    /// file exists yet (normal on first visit to a track).
    pub fn load(dir: &Path, track_name: &str, track_length_m: f32) -> Option<Self> {
        let path = dir.join(format!("{}.json", make_safe_name(track_name, track_length_m)));
        let json = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&json).ok()
    }

    /// Returns the corner that *contains* the given track position, i.e. where
    /// `brake_point <= pos <= exit`. Returns `None` on a straight.
    pub fn corner_at(&self, track_pos: f32) -> Option<&DetectedCorner> {
        self.corners
            .iter()
            .find(|c| track_pos >= c.brake_point && track_pos <= c.exit)
    }

    pub fn corner_by_id(&self, id: u8) -> Option<&DetectedCorner> {
        self.corners.iter().find(|c| c.id == id)
    }

    #[allow(dead_code)]
    pub fn corner_by_id_mut(&mut self, id: u8) -> Option<&mut DetectedCorner> {
        self.corners.iter_mut().find(|c| c.id == id)
    }
}

fn make_safe_name(track_name: &str, track_length_m: f32) -> String {
    let safe: String = track_name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    // Only include length in the stem when it's non-zero.
    if track_length_m > 0.0 {
        format!("{}_{:.0}m", safe, track_length_m)
    } else {
        safe
    }
}
