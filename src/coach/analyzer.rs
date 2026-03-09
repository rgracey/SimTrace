//! Rules-based event detection.
//!
//! The `Analyzer` has two entry points:
//!
//! * `analyze_realtime` — called on every new sample; detects global patterns
//!   (ABS abuse, TC abuse, coasting, overlap, steering saw).
//! * `analyze_corner` — called once when a corner's exit point is passed;
//!   compares this lap's corner performance to the reference.
//!
//! Both return `Vec<StructuredTip>`. The coach thread decides which tip to
//! send based on priority and cooldown.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::centerline::Centerline;
use super::corner::{CornerDirection, DetectedCorner};
use super::events::{CoachEvent, StructuredTip};
use super::lap::LapSample;
use super::reference::CornerPerf;

// ── Thresholds ────────────────────────────────────────────────────────────────

/// Both pedals above this fraction → overlap (excludes gear-blip noise).
const OVERLAP_THRESHOLD: f32 = 0.10;
/// Overlap must persist this long before a tip fires.
const OVERLAP_MIN_MS: u32 = 500;

/// Below this speed coasting is normal (low-speed hairpin exit etc.).
const COAST_MIN_SPEED_KPH: f32 = 80.0;
/// Coasting must exceed this duration before a tip fires.
const COAST_MIN_MS: u32 = 2_000;

const SAW_WINDOW_SECS: u64 = 1;
const SAW_THRESHOLD_PER_SEC: f32 = 3.0;
const SAW_MIN_STEER_DEG: f32 = 5.0;

/// Apex speed must differ by at least this much to trigger a tip (kph).
const APEX_SPEED_THRESHOLD_KPH: f32 = 8.0;
/// Minimum brake-point delta in metres to trigger an early/late braking tip.
const BRAKE_DELTA_MIN_M: f32 = 5.0;
/// Entry speed above reference by this to trigger "too late" tip (kph).
const ENTRY_SPEED_HOT_KPH: f32 = 10.0;
/// Minimum throttle-point delta in metres to trigger a tip.
const THROTTLE_DELTA_MIN_M: f32 = 5.0;
/// TC activations during a corner exit to flag early throttle.
const TC_CORNER_THRESHOLD: u32 = 2;
/// Minimum metres ahead of geometric apex for driver's speed minimum to count as early apexing.
const EARLY_APEX_MIN_M: f32 = 15.0;
/// Minimum world-space metres off the centerline at the apex to flag a missed apex.
const MISSED_APEX_MIN_M: f32 = 4.0;
/// Steering angle (degrees) at throttle application that suggests exit understeer.
const EXIT_UNDERSTEER_STEER_DEG: f32 = 12.0;

/// Round a distance in metres to the nearest 10 m (minimum 10 m).
/// Turns "37m" into "40m" — more natural for a driver to act on.
fn round_m(m: f32) -> u32 {
    ((m / 10.0).round() as u32).max(1) * 10
}

// ── Analyzer ─────────────────────────────────────────────────────────────────

pub struct Analyzer {
    overlap_since: Option<Instant>,
    coast_since: Option<Instant>,

    last_steering_sign: Option<f32>,
    steering_reversals: VecDeque<Instant>,
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            overlap_since: None,
            coast_since: None,
            last_steering_sign: None,
            steering_reversals: VecDeque::new(),
        }
    }

    /// Analyse one real-time sample. Returns zero or more tips.
    ///
    /// Tips returned here are global (not corner-specific). The coach thread
    /// applies cooldown before forwarding any of them.
    pub fn analyze_realtime(&mut self, sample: &LapSample) -> Vec<StructuredTip> {
        let mut tips = Vec::new();

        // ── Throttle / brake overlap ─────────────────────────────────────────
        let overlapping = sample.throttle > OVERLAP_THRESHOLD && sample.brake > OVERLAP_THRESHOLD;
        if overlapping {
            self.overlap_since.get_or_insert(Instant::now());
        } else if let Some(start) = self.overlap_since.take() {
            let ms = start.elapsed().as_millis() as u32;
            if ms > OVERLAP_MIN_MS {
                tips.push(StructuredTip::new(
                    CoachEvent::ThrottleBrakeOverlap { overlap_ms: ms },
                    "Clean pedal inputs — brake and throttle overlapping".to_string(),
                    2,
                    None,
                ));
            }
        }

        // ── Excessive coasting ───────────────────────────────────────────────
        let coasting =
            sample.throttle < 0.02 && sample.brake < 0.02 && sample.speed_kph > COAST_MIN_SPEED_KPH;
        if coasting {
            self.coast_since.get_or_insert(Instant::now());
        } else if let Some(start) = self.coast_since.take() {
            let ms = start.elapsed().as_millis() as u32;
            if ms > COAST_MIN_MS {
                tips.push(StructuredTip::new(
                    CoachEvent::CoastingExcessive {
                        duration_ms: ms,
                        speed_kph: sample.speed_kph,
                    },
                    "Don't lift — trail brake instead".to_string(),
                    2,
                    None,
                ));
            }
        }

        // ── Steering saw ─────────────────────────────────────────────────────
        let sign = if sample.steering_angle.abs() > SAW_MIN_STEER_DEG {
            Some(sample.steering_angle.signum())
        } else {
            None
        };
        if let (Some(prev), Some(curr)) = (self.last_steering_sign, sign) {
            if (prev - curr).abs() > 0.5 {
                // Direction changed.
                self.steering_reversals.push_back(Instant::now());
                let cutoff = Instant::now() - Duration::from_secs(SAW_WINDOW_SECS);
                while self
                    .steering_reversals
                    .front()
                    .map(|&t| t < cutoff)
                    .unwrap_or(false)
                {
                    self.steering_reversals.pop_front();
                }
                let rps = self.steering_reversals.len() as f32;
                if rps > SAW_THRESHOLD_PER_SEC {
                    tips.push(StructuredTip::new(
                        CoachEvent::SteeringSaw {
                            reversals_per_sec: rps,
                        },
                        "Smooth inputs — steering too aggressive".to_string(),
                        2,
                        None,
                    ));
                }
            }
        }
        self.last_steering_sign = sign;

        tips
    }

    /// Analyse corner performance after the driver passes a corner's zone_exit.
    ///
    /// `corner_samples` are the samples collected while the driver was inside
    /// the corner's geometric zone (turn_in..zone_exit).  Returns tips
    /// comparing this lap's performance to `reference`, or an empty vec if no
    /// reference is provided.
    ///
    /// `centerline` is optional — when present and world XZ is available,
    /// line-deviation tips ("you're off the apex") are also generated.
    pub fn analyze_corner(
        &self,
        corner: &DetectedCorner,
        corner_samples: &[LapSample],
        reference: Option<&CornerPerf>,
        track_length_m: f32,
        centerline: Option<&Centerline>,
    ) -> Vec<StructuredTip> {
        let mut tips = Vec::new();

        if corner_samples.is_empty() {
            return tips;
        }

        let dir_str = match corner.direction {
            CornerDirection::Left => "left",
            CornerDirection::Right => "right",
        };
        let t = corner.id; // shorthand for tip formatting

        // ── Find actual apex (speed minimum) ─────────────────────────────────
        let apex_sample = corner_samples
            .iter()
            .min_by(|a, b| {
                a.speed_kph
                    .partial_cmp(&b.speed_kph)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        let apex_speed = apex_sample.map(|s| s.speed_kph).unwrap_or(f32::INFINITY);
        let actual_apex_pos = apex_sample.map(|s| s.track_pos).unwrap_or(corner.apex);

        // ── Early apex ───────────────────────────────────────────────────────
        // Driver's min-speed point is well before the geometric apex.
        // This is the most common cause of running wide on exit.
        let early_m = (corner.apex - actual_apex_pos) * track_length_m;
        if early_m > EARLY_APEX_MIN_M {
            tips.push(StructuredTip::new(
                CoachEvent::EarlyApex {
                    corner_id: t,
                    early_m,
                },
                format!(
                    "Turn {t} ({dir_str}): you're hitting the apex {}m too early — \
                     wait for the geometric apex or you'll run wide on exit",
                    round_m(early_m)
                ),
                4,
                Some(t),
            ));
        }

        // ── Brake over apex ──────────────────────────────────────────────────
        // Brake trace still active past the geometric apex prevents the car
        // from rotating and causes understeer / slow exit.
        let braking_past_apex = corner_samples
            .iter()
            .any(|s| s.track_pos > corner.apex && s.brake > 0.05);
        if braking_past_apex {
            tips.push(StructuredTip::new(
                CoachEvent::BrakeOverApex { corner_id: t },
                format!(
                    "Turn {t} ({dir_str}): release the brakes before the apex — \
                     you're still on them mid-corner and killing rotation"
                ),
                3,
                Some(t),
            ));
        }

        // ── World-XZ apex deviation ──────────────────────────────────────────
        // When world coordinates are available, compare the driver's position
        // at the apex to the ideal line (centerline).
        if let (Some(cl), Some(apex_s)) = (centerline, apex_sample) {
            if apex_s.world_x != 0.0 {
                if let Some(cl_pt) = cl.point_at(corner.apex) {
                    let dx = apex_s.world_x - cl_pt.x;
                    let dz = apex_s.world_z - cl_pt.z;
                    let deviation_m = (dx * dx + dz * dz).sqrt();
                    if deviation_m > MISSED_APEX_MIN_M {
                        tips.push(StructuredTip::new(
                            CoachEvent::MissedApex {
                                corner_id: t,
                                deviation_m,
                            },
                            format!(
                                "Turn {t} ({dir_str}): you're {}m off the apex — \
                                 get closer to the inside",
                                deviation_m as u32
                            ),
                            4,
                            Some(t),
                        ));
                    }
                }
            }
        }

        // From here on, tips require a reference lap.
        let Some(ref_perf) = reference else {
            return tips;
        };

        // Track whether a root-cause tip already explains poor apex speed,
        // so we don't double-tip "slow apex" as a separate item.
        let mut apex_explained = !tips.is_empty();

        let apex_delta = ref_perf.apex_speed_kph - apex_speed;

        // ── Gear at apex ─────────────────────────────────────────────────────
        let apex_gear = apex_sample.map(|s| s.gear).unwrap_or(0);
        if apex_gear > 0 && ref_perf.gear > 0 && apex_gear != ref_perf.gear {
            let gear_tip = if apex_gear > ref_perf.gear {
                format!(
                    "Turn {t} ({dir_str}): drop to gear {} at the apex — \
                     you're a gear too high and losing drive",
                    ref_perf.gear
                )
            } else {
                format!(
                    "Turn {t} ({dir_str}): try gear {} at the apex — \
                     you're over-revving through the corner",
                    ref_perf.gear
                )
            };
            tips.push(StructuredTip::new(
                CoachEvent::WrongGearAtApex {
                    corner_id: t,
                    actual_gear: apex_gear,
                    ref_gear: ref_perf.gear,
                },
                gear_tip,
                3,
                Some(t),
            ));
            apex_explained = true;
        }

        // ── Brake point ──────────────────────────────────────────────────────
        let actual_brake_point = corner_samples
            .iter()
            .find(|s| s.brake > 0.05)
            .map(|s| s.track_pos);

        if let Some(actual_bp) = actual_brake_point {
            let delta = actual_bp - ref_perf.brake_point;
            let delta_m = delta * track_length_m;

            if delta_m > BRAKE_DELTA_MIN_M {
                // Braking before the reference point.
                apex_explained = true;
                tips.push(StructuredTip::new(
                    CoachEvent::BrakingTooEarly {
                        corner_id: t,
                        delta_track_pos: delta,
                        estimated_time_lost_ms: delta_m * 10.0,
                    },
                    format!(
                        "Turn {t} ({dir_str}): brake {}m later — \
                         you're giving up {}kph of entry speed",
                        round_m(delta_m),
                        (apex_delta.max(0.0) as u32)
                    ),
                    3,
                    Some(t),
                ));
            } else if delta_m < -BRAKE_DELTA_MIN_M {
                // Braking after the reference — hot entry.
                let entry_speed = corner_samples.first().map(|s| s.speed_kph).unwrap_or(0.0);
                let speed_delta = entry_speed - ref_perf.entry_speed_kph;
                if speed_delta > ENTRY_SPEED_HOT_KPH {
                    apex_explained = true;
                    tips.push(StructuredTip::new(
                        CoachEvent::BrakingTooLate {
                            corner_id: t,
                            entry_speed_delta_kph: speed_delta,
                        },
                        format!(
                            "Turn {t} ({dir_str}): brake {}m earlier — \
                             you're arriving {:.0}kph too hot and running wide",
                            round_m(-delta_m),
                            speed_delta
                        ),
                        4,
                        Some(t),
                    ));
                }
            }
        }

        // ── Throttle application ─────────────────────────────────────────────
        let actual_throttle_point = corner_samples
            .iter()
            .find(|s| s.track_pos > corner.apex && s.throttle > 0.20);

        if let Some(actual_tp_s) = actual_throttle_point {
            let actual_tp = actual_tp_s.track_pos;
            let delta = actual_tp - ref_perf.throttle_point;
            let delta_m = delta * track_length_m;

            if delta_m > THROTTLE_DELTA_MIN_M {
                apex_explained = true;
                tips.push(StructuredTip::new(
                    CoachEvent::ThrottleTooLate {
                        corner_id: t,
                        delta_track_pos: delta,
                    },
                    format!(
                        "Turn {t} ({dir_str}): power up {}m earlier on the way out — \
                         you're leaving exit speed on the table",
                        round_m(delta_m)
                    ),
                    3,
                    Some(t),
                ));
            } else if delta_m < -THROTTLE_DELTA_MIN_M {
                let tc_hits = corner_samples
                    .windows(2)
                    .filter(|w| !w[0].tc_active && w[1].tc_active)
                    .count() as u32;
                if tc_hits > TC_CORNER_THRESHOLD {
                    apex_explained = true;
                    tips.push(StructuredTip::new(
                        CoachEvent::ThrottleTooEarly { corner_id: t },
                        format!(
                            "Turn {t} ({dir_str}): wait longer before powering — \
                             TC fired {tc_hits}× on exit, you're spinning the wheels"
                        ),
                        3,
                        Some(t),
                    ));
                }
            }

            // ── Exit understeer ──────────────────────────────────────────────
            // High steering angle at the point of throttle application means
            // the car is still pointing at the outside — understeer / wrong line.
            if actual_tp_s.steering_angle.abs() > EXIT_UNDERSTEER_STEER_DEG {
                tips.push(StructuredTip::new(
                    CoachEvent::ExitUndersteer { corner_id: t },
                    format!(
                        "Turn {t} ({dir_str}): wait until the car is pointed straight \
                         before powering — you're fighting understeer on exit"
                    ),
                    3,
                    Some(t),
                ));
            }
        }

        // ── Apex speed — only when no root-cause tip already explains it ─────
        if !apex_explained && apex_delta > APEX_SPEED_THRESHOLD_KPH {
            tips.push(StructuredTip::new(
                CoachEvent::SlowApex {
                    corner_id: t,
                    delta_kph: apex_delta,
                },
                format!(
                    "Turn {t} ({dir_str}): you're {:.0}kph down at the apex — \
                     commit to the inside and carry more speed",
                    apex_delta
                ),
                4,
                Some(t),
            ));
        }

        tips
    }
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}
