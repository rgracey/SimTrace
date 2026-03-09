//! Rules-based event detection.
//!
//! The `Analyzer` has two entry points:
//!
//! * `analyze_realtime` — called on every new sample; detects global patterns
//!   (coasting, overlap, steering saw).
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

/// Apex speed must differ by at least this much to trigger a generic "slow apex" tip (kph).
const APEX_SPEED_THRESHOLD_KPH: f32 = 8.0;
/// Minimum brake-point delta in metres to trigger an early/late braking tip.
const BRAKE_DELTA_MIN_M: f32 = 5.0;
/// Entry speed above reference by this much to trigger a "too late" tip (kph).
const ENTRY_SPEED_HOT_KPH: f32 = 10.0;
/// Minimum throttle-point delta in metres to trigger a tip.
const THROTTLE_DELTA_MIN_M: f32 = 5.0;
/// TC activations during corner exit to flag early throttle application.
const TC_CORNER_THRESHOLD: u32 = 2;
/// Minimum metres ahead of geometric apex for driver's speed minimum to count as early apexing.
const EARLY_APEX_MIN_M: f32 = 15.0;
/// Minimum world-space metres off the centerline at the apex to flag a missed apex.
const MISSED_APEX_MIN_M: f32 = 4.0;
/// Steering angle (degrees) at throttle application that suggests exit understeer.
const EXIT_UNDERSTEER_STEER_DEG: f32 = 12.0;
/// Maximum single-sample brake-drop (0–1 normalised) before considering trail brake too sudden.
const TRAIL_BRAKE_SUDDEN_DROP: f32 = 0.20;
/// Distance (m) from 20% to 80% throttle above which buildup is flagged as too slow.
const THROTTLE_BUILDUP_SLOW_M: f32 = 60.0;
/// Distance (m) of sustained partial throttle (20–55%) before flagging hesitation.
const THROTTLE_HESITATION_M: f32 = 30.0;

// ── Formatting helpers ────────────────────────────────────────────────────────

/// Format a distance as a range string (e.g. "10–20m") to acknowledge measurement noise.
/// Values ≤ 10 m are shown exactly; larger values get a ±5 m band.
fn dist_str(m: f32) -> String {
    let m = m.max(5.0);
    let mid = ((m / 5.0).round() as u32).max(1) * 5;
    if mid <= 10 {
        format!("{mid}m")
    } else {
        format!("{}–{}m", mid - 5, mid + 5)
    }
}

// ── Time-loss estimation ──────────────────────────────────────────────────────

/// Estimate seconds lost in this corner vs reference using a two-segment speed model.
///
/// The corner is split at the geometric apex into:
/// * Entry zone (turn_in → apex): interpolates entry→apex speed.
/// * Exit zone (apex → zone_exit): interpolates apex→exit speed.
///
/// Time = 2·d / (v₁ + v₂) for each linear segment.
fn estimate_time_loss_s(
    ref_perf: &CornerPerf,
    corner: &DetectedCorner,
    corner_samples: &[LapSample],
    track_length_m: f32,
) -> Option<f32> {
    let apex_m = (corner.apex - corner.turn_in).abs() * track_length_m;
    let exit_m = (corner.zone_exit - corner.apex).abs() * track_length_m;
    if apex_m < 1.0 || exit_m < 1.0 {
        return None;
    }

    let actual_entry = corner_samples.first()?.speed_kph;
    let actual_apex = corner_samples
        .iter()
        .map(|s| s.speed_kph)
        .fold(f32::INFINITY, f32::min);
    let actual_exit = corner_samples.last()?.speed_kph;
    if !actual_apex.is_finite() {
        return None;
    }

    fn seg_time(dist_m: f32, v1_kph: f32, v2_kph: f32) -> f32 {
        let avg_ms = ((v1_kph + v2_kph) / 2.0) / 3.6;
        if avg_ms > 0.1 { dist_m / avg_ms } else { 0.0 }
    }

    let ref_time = seg_time(apex_m, ref_perf.entry_speed_kph, ref_perf.apex_speed_kph)
        + seg_time(exit_m, ref_perf.apex_speed_kph, ref_perf.exit_speed_kph);
    let actual_time = seg_time(apex_m, actual_entry, actual_apex)
        + seg_time(exit_m, actual_apex, actual_exit);

    let loss = actual_time - ref_time;
    if loss > 0.02 { Some(loss.min(3.0)) } else { None }
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

    /// Analyse one real-time sample. Returns zero or more global tips.
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
                        CoachEvent::SteeringSaw { reversals_per_sec: rps },
                        "Steering corrections are causing instability".to_string(),
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
    /// the corner's geometric zone (turn_in..zone_exit). Returns tips
    /// comparing this lap's performance to `reference`.
    ///
    /// `centerline` is optional — when present and world XZ is available,
    /// line-deviation tips are also generated.
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

        let t = corner.id;
        let dir = match corner.direction {
            CornerDirection::Left => "left",
            CornerDirection::Right => "right",
        };

        // ── Find actual apex (speed minimum) and time loss ────────────────────
        let apex_sample = corner_samples.iter().min_by(|a, b| {
            a.speed_kph
                .partial_cmp(&b.speed_kph)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let apex_speed = apex_sample.map(|s| s.speed_kph).unwrap_or(f32::INFINITY);
        let actual_apex_pos = apex_sample.map(|s| s.track_pos).unwrap_or(corner.apex);

        let time_loss = reference
            .and_then(|r| estimate_time_loss_s(r, corner, corner_samples, track_length_m));

        // ── Early apex (turning in too early) ────────────────────────────────
        // Speed minimum well before the geometric apex → will run wide on exit.
        let early_m = (corner.apex - actual_apex_pos) * track_length_m;
        if early_m > EARLY_APEX_MIN_M {
            tips.push(
                StructuredTip::new(
                    CoachEvent::EarlyApex { corner_id: t, early_m },
                    format!("Turn {t}: you're turning in too early ({dir}) — turn in {d} later.",
                        d = dist_str(early_m)),
                    4,
                    Some(t),
                )
                .with_time_loss(time_loss),
            );
        }

        // ── Brake released past the geometric apex ────────────────────────────
        let braking_past_apex = corner_samples
            .iter()
            .any(|s| s.track_pos > corner.apex && s.brake > 0.05);
        if braking_past_apex {
            tips.push(StructuredTip::new(
                CoachEvent::BrakeOverApex { corner_id: t },
                format!("Turn {t}: release the brake earlier into the corner ({dir})."),
                3,
                Some(t),
            ));
        }

        // ── Trail brake quality ───────────────────────────────────────────────
        // If the brake drops sharply rather than trailing off, flag it.
        // Only check if the driver is actually trail braking (brake > 0 near apex).
        {
            let brake_vals: Vec<f32> = corner_samples
                .iter()
                .filter(|s| s.brake > 0.02 && s.track_pos <= corner.apex)
                .map(|s| s.brake)
                .collect();

            if brake_vals.len() >= 5 {
                let deltas: Vec<f32> = brake_vals
                    .windows(2)
                    .map(|w| (w[0] - w[1]).max(0.0)) // release rate per sample
                    .collect();
                let avg = deltas.iter().sum::<f32>() / deltas.len() as f32;
                let max_drop = deltas.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                // Sudden if one sample drops > 3× the average AND > absolute threshold.
                if avg > 0.005 && max_drop > 3.0 * avg && max_drop > TRAIL_BRAKE_SUDDEN_DROP {
                    tips.push(StructuredTip::new(
                        CoachEvent::TrailBrakeTooSudden { corner_id: t },
                        format!("Turn {t}: trail brake more gradually ({dir})."),
                        3,
                        Some(t),
                    ));
                }
            }
        }

        // ── World-XZ apex deviation ───────────────────────────────────────────
        if let (Some(cl), Some(apex_s)) = (centerline, apex_sample) {
            if apex_s.world_x != 0.0 {
                if let Some(cl_pt) = cl.point_at(corner.apex) {
                    let dx = apex_s.world_x - cl_pt.x;
                    let dz = apex_s.world_z - cl_pt.z;
                    let deviation_m = (dx * dx + dz * dz).sqrt();
                    if deviation_m > MISSED_APEX_MIN_M {
                        tips.push(
                            StructuredTip::new(
                                CoachEvent::MissedApex { corner_id: t, deviation_m },
                                format!("Turn {t}: you're missing the apex — get closer to the inside ({dir})."),
                                4,
                                Some(t),
                            )
                            .with_time_loss(time_loss),
                        );
                    }
                }
            }
        }

        // From here on, tips require a reference lap.
        let Some(ref_perf) = reference else {
            return tips;
        };

        // Track whether a root-cause tip already explains poor apex speed,
        // so we don't also fire the generic "carry more speed" tip.
        let mut apex_explained = !tips.is_empty();
        let apex_delta = ref_perf.apex_speed_kph - apex_speed;

        // ── Per-corner ABS activations ────────────────────────────────────────
        let abs_hits = corner_samples
            .windows(2)
            .filter(|w| !w[0].abs_active && w[1].abs_active)
            .count() as u32;
        if abs_hits > 0 && abs_hits > ref_perf.abs_activations {
            tips.push(StructuredTip::new(
                CoachEvent::ExcessiveBrakePressure { corner_id: t, abs_count: abs_hits },
                format!("Turn {t}: reduce peak brake pressure ({dir})."),
                3,
                Some(t),
            ));
        }

        // ── Gear at apex ──────────────────────────────────────────────────────
        let apex_gear = apex_sample.map(|s| s.gear).unwrap_or(0);
        if apex_gear > 0 && ref_perf.gear > 0 && apex_gear != ref_perf.gear {
            let gear_tip = if apex_gear > ref_perf.gear {
                format!("Turn {t}: take the corner in gear {g} ({dir}).", g = ref_perf.gear)
            } else {
                format!("Turn {t}: try gear {g} at the apex ({dir}) — you're over-revving.", g = ref_perf.gear)
            };
            tips.push(
                StructuredTip::new(
                    CoachEvent::WrongGearAtApex {
                        corner_id: t,
                        actual_gear: apex_gear,
                        ref_gear: ref_perf.gear,
                    },
                    gear_tip,
                    3,
                    Some(t),
                )
                .with_time_loss(time_loss),
            );
            apex_explained = true;
        }

        // ── Brake point ───────────────────────────────────────────────────────
        let actual_brake_point = corner_samples
            .iter()
            .find(|s| s.brake > 0.05)
            .map(|s| s.track_pos);

        if let Some(actual_bp) = actual_brake_point {
            let delta_m = (actual_bp - ref_perf.brake_point) * track_length_m;

            if delta_m > BRAKE_DELTA_MIN_M {
                // Braking earlier than reference.
                apex_explained = true;
                tips.push(
                    StructuredTip::new(
                        CoachEvent::BrakingTooEarly {
                            corner_id: t,
                            delta_track_pos: delta_m / track_length_m,
                        },
                        format!("Brake {d} later for Turn {t}.", d = dist_str(delta_m)),
                        3,
                        Some(t),
                    )
                    .with_time_loss(time_loss),
                );
            } else if delta_m < -BRAKE_DELTA_MIN_M {
                // Braking after the reference — hot entry.
                let entry_speed = corner_samples.first().map(|s| s.speed_kph).unwrap_or(0.0);
                let speed_delta = entry_speed - ref_perf.entry_speed_kph;
                if speed_delta > ENTRY_SPEED_HOT_KPH {
                    apex_explained = true;
                    tips.push(
                        StructuredTip::new(
                            CoachEvent::BrakingTooLate {
                                corner_id: t,
                                entry_speed_delta_kph: speed_delta,
                            },
                            format!(
                                "Turn {t}: carry less speed into the corner ({dir}) — brake {d} earlier.",
                                d = dist_str(-delta_m)
                            ),
                            4,
                            Some(t),
                        )
                        .with_time_loss(time_loss),
                    );
                }
            }
        }

        // ── Throttle application ──────────────────────────────────────────────
        let actual_throttle_sample = corner_samples
            .iter()
            .find(|s| s.track_pos > corner.apex && s.throttle > 0.20);

        if let Some(tp_s) = actual_throttle_sample {
            let delta_m = (tp_s.track_pos - ref_perf.throttle_point) * track_length_m;

            if delta_m > THROTTLE_DELTA_MIN_M {
                apex_explained = true;
                tips.push(
                    StructuredTip::new(
                        CoachEvent::ThrottleTooLate {
                            corner_id: t,
                            delta_track_pos: delta_m / track_length_m,
                        },
                        format!("Turn {t}: apply throttle {d} earlier on exit ({dir}).",
                            d = dist_str(delta_m)),
                        3,
                        Some(t),
                    )
                    .with_time_loss(time_loss),
                );
            } else if delta_m < -THROTTLE_DELTA_MIN_M {
                let tc_hits = corner_samples
                    .windows(2)
                    .filter(|w| !w[0].tc_active && w[1].tc_active)
                    .count() as u32;
                if tc_hits > TC_CORNER_THRESHOLD {
                    apex_explained = true;
                    tips.push(StructuredTip::new(
                        CoachEvent::ThrottleTooEarly { corner_id: t },
                        format!("Turn {t}: wait until the car is pointed straight before powering ({dir})."),
                        3,
                        Some(t),
                    ));
                }
            }

            // ── Exit understeer ───────────────────────────────────────────────
            if tp_s.steering_angle.abs() > EXIT_UNDERSTEER_STEER_DEG {
                tips.push(StructuredTip::new(
                    CoachEvent::ExitUndersteer { corner_id: t },
                    format!("Turn {t}: use more track on exit ({dir}) — rotate the car before powering."),
                    3,
                    Some(t),
                ));
            }

            // ── Throttle build-up quality ─────────────────────────────────────
            // Check how far it takes to go from 20% to 80% throttle.
            let exit_samples: Vec<&LapSample> = corner_samples
                .iter()
                .filter(|s| s.track_pos >= tp_s.track_pos)
                .collect();

            let buildup_m = exit_samples
                .iter()
                .find(|s| s.throttle >= 0.80)
                .map(|s| (s.track_pos - tp_s.track_pos) * track_length_m);

            if let Some(bm) = buildup_m {
                if bm > THROTTLE_BUILDUP_SLOW_M {
                    tips.push(StructuredTip::new(
                        CoachEvent::ThrottleBuildupSlow { corner_id: t },
                        format!("Turn {t}: throttle build-up is too slow on exit ({dir})."),
                        2,
                        Some(t),
                    ));
                }
            }

            // ── Throttle hesitation ───────────────────────────────────────────
            // Prolonged partial throttle (20–55%) before committing.
            let hesitation_m = exit_samples
                .iter()
                .take_while(|s| s.throttle > 0.15 && s.throttle < 0.55)
                .last()
                .map(|s| (s.track_pos - tp_s.track_pos) * track_length_m)
                .unwrap_or(0.0);

            if hesitation_m > THROTTLE_HESITATION_M {
                tips.push(StructuredTip::new(
                    CoachEvent::ThrottleHesitation { corner_id: t },
                    format!("Turn {t}: you're hesitating before full throttle — commit earlier ({dir})."),
                    2,
                    Some(t),
                ));
            }
        }

        // ── Apex speed — only when no root-cause tip explains it ─────────────
        if !apex_explained && apex_delta > APEX_SPEED_THRESHOLD_KPH {
            tips.push(
                StructuredTip::new(
                    CoachEvent::SlowApex { corner_id: t, delta_kph: apex_delta },
                    format!("Turn {t}: carry more speed through the corner ({dir})."),
                    4,
                    Some(t),
                )
                .with_time_loss(time_loss),
            );
        }

        tips
    }
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}
