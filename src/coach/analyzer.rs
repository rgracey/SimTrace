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

use super::corner::DetectedCorner;
use super::events::{CoachEvent, StructuredTip};
use super::lap::LapSample;
use super::reference::CornerPerf;

// ── Thresholds ────────────────────────────────────────────────────────────────

const ABS_WINDOW_SECS: u64 = 15;
const ABS_ABUSE_THRESHOLD: u32 = 4;

const TC_WINDOW_SECS: u64 = 15;
const TC_ABUSE_THRESHOLD: u32 = 4;

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

/// Round a distance in metres to the nearest 10 m (minimum 10 m).
/// Turns "37m" into "40m" — more natural for a driver to act on.
fn round_m(m: f32) -> u32 {
    ((m / 10.0).round() as u32).max(1) * 10
}

// ── Rolling event window ──────────────────────────────────────────────────────

struct EventWindow {
    events: VecDeque<Instant>,
    window: Duration,
}

impl EventWindow {
    fn new(window_secs: u64) -> Self {
        Self {
            events: VecDeque::new(),
            window: Duration::from_secs(window_secs),
        }
    }

    fn record(&mut self) {
        self.prune();
        self.events.push_back(Instant::now());
    }

    fn count(&mut self) -> u32 {
        self.prune();
        self.events.len() as u32
    }

    fn prune(&mut self) {
        let cutoff = Instant::now() - self.window;
        while self.events.front().map(|&t| t < cutoff).unwrap_or(false) {
            self.events.pop_front();
        }
    }
}

// ── Analyzer ─────────────────────────────────────────────────────────────────

pub struct Analyzer {
    abs_window: EventWindow,
    tc_window: EventWindow,
    was_abs_active: bool,
    was_tc_active: bool,

    overlap_since: Option<Instant>,
    coast_since: Option<Instant>,

    last_steering_sign: Option<f32>,
    steering_reversals: VecDeque<Instant>,
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            abs_window: EventWindow::new(ABS_WINDOW_SECS),
            tc_window: EventWindow::new(TC_WINDOW_SECS),
            was_abs_active: false,
            was_tc_active: false,
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

        // ── ABS activation tracking ──────────────────────────────────────────
        if sample.abs_active && !self.was_abs_active {
            self.abs_window.record();
        }
        self.was_abs_active = sample.abs_active;

        let abs_count = self.abs_window.count();
        if abs_count > ABS_ABUSE_THRESHOLD {
            tips.push(StructuredTip::new(
                CoachEvent::AbsAbuse {
                    count: abs_count,
                    window_secs: ABS_WINDOW_SECS as f32,
                },
                format!(
                    "ABS has activated {} times in {} seconds — \
                     you're arriving at corners with too much brake pressure.",
                    abs_count, ABS_WINDOW_SECS
                ),
                3,
                None,
            ));
        }

        // ── TC activation tracking ───────────────────────────────────────────
        if sample.tc_active && !self.was_tc_active {
            self.tc_window.record();
        }
        self.was_tc_active = sample.tc_active;

        let tc_count = self.tc_window.count();
        if tc_count > TC_ABUSE_THRESHOLD {
            tips.push(StructuredTip::new(
                CoachEvent::TcAbuse {
                    count: tc_count,
                    window_secs: TC_WINDOW_SECS as f32,
                },
                format!(
                    "Traction control has fired {} times in {} seconds — \
                     try applying throttle later or more progressively on corner exits.",
                    tc_count, TC_WINDOW_SECS
                ),
                3,
                None,
            ));
        }

        // ── Throttle / brake overlap ─────────────────────────────────────────
        let overlapping = sample.throttle > OVERLAP_THRESHOLD && sample.brake > OVERLAP_THRESHOLD;
        if overlapping {
            self.overlap_since.get_or_insert(Instant::now());
        } else if let Some(start) = self.overlap_since.take() {
            let ms = start.elapsed().as_millis() as u32;
            if ms > OVERLAP_MIN_MS {
                tips.push(StructuredTip::new(
                    CoachEvent::ThrottleBrakeOverlap { overlap_ms: ms },
                    format!(
                        "Throttle and brake were both active for {} ms — \
                         fully release one pedal before applying the other.",
                        ms
                    ),
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
                    format!(
                        "You coasted for {} ms at {:.0} kph with no pedal input — \
                         carry that momentum with trail braking rather than lifting early.",
                        ms, sample.speed_kph
                    ),
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
                        format!(
                            "Steering is reversing direction {:.0} times per second — \
                             commit to smoother, more deliberate inputs.",
                            rps
                        ),
                        2,
                        None,
                    ));
                }
            }
        }
        self.last_steering_sign = sign;

        tips
    }

    /// Analyse corner performance after the driver passes a corner's exit point.
    ///
    /// `corner_samples` must be the samples collected between the corner's
    /// `brake_point` and `exit` track positions. Returns tips comparing this
    /// performance to `reference`, or an empty vec if no reference is provided.
    pub fn analyze_corner(
        &self,
        corner: &DetectedCorner,
        corner_samples: &[LapSample],
        reference: Option<&CornerPerf>,
        track_length_m: f32,
    ) -> Vec<StructuredTip> {
        let mut tips = Vec::new();

        let Some(ref_perf) = reference else {
            return tips;
        };

        if corner_samples.is_empty() {
            return tips;
        }

        // Track whether a root-cause tip already explains poor apex speed,
        // so we don't double-tip "slow apex" as a separate item.
        let mut brake_or_throttle_tip_fired = false;

        // ── Apex speed ───────────────────────────────────────────────────────
        let apex_speed = corner_samples
            .iter()
            .map(|s| s.speed_kph)
            .fold(f32::INFINITY, f32::min);

        let apex_delta = ref_perf.apex_speed_kph - apex_speed;

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
                brake_or_throttle_tip_fired = true;
                tips.push(StructuredTip::new(
                    CoachEvent::BrakingTooEarly {
                        corner_id: corner.id,
                        delta_track_pos: delta,
                        estimated_time_lost_ms: delta_m * 10.0,
                    },
                    format!(
                        "Corner {}: brake {}m later.",
                        corner.id, round_m(delta_m)
                    ),
                    3,
                    Some(corner.id),
                ));
            } else if delta_m < -BRAKE_DELTA_MIN_M {
                // Braking after the reference — hot entry.
                let entry_speed = corner_samples.first().map(|s| s.speed_kph).unwrap_or(0.0);
                let speed_delta = entry_speed - ref_perf.entry_speed_kph;
                if speed_delta > ENTRY_SPEED_HOT_KPH {
                    brake_or_throttle_tip_fired = true;
                    tips.push(StructuredTip::new(
                        CoachEvent::BrakingTooLate {
                            corner_id: corner.id,
                            entry_speed_delta_kph: speed_delta,
                        },
                        format!(
                            "Corner {}: entry is {:.0} kph too fast — move the brake point {}m earlier.",
                            corner.id, speed_delta, round_m(-delta_m)
                        ),
                        4,
                        Some(corner.id),
                    ));
                }
            }
        }

        // ── Throttle application ─────────────────────────────────────────────
        let actual_throttle_point = corner_samples
            .iter()
            .find(|s| s.track_pos > corner.apex && s.throttle > 0.20)
            .map(|s| s.track_pos);

        if let Some(actual_tp) = actual_throttle_point {
            let delta = actual_tp - ref_perf.throttle_point;
            let delta_m = delta * track_length_m;

            if delta_m > THROTTLE_DELTA_MIN_M {
                brake_or_throttle_tip_fired = true;
                tips.push(StructuredTip::new(
                    CoachEvent::ThrottleTooLate {
                        corner_id: corner.id,
                        delta_track_pos: delta,
                    },
                    format!(
                        "Corner {}: get on the power {}m earlier on exit.",
                        corner.id, round_m(delta_m)
                    ),
                    3,
                    Some(corner.id),
                ));
            } else if delta_m < -THROTTLE_DELTA_MIN_M {
                let tc_hits = corner_samples
                    .windows(2)
                    .filter(|w| !w[0].tc_active && w[1].tc_active)
                    .count() as u32;
                if tc_hits > TC_CORNER_THRESHOLD {
                    brake_or_throttle_tip_fired = true;
                    tips.push(StructuredTip::new(
                        CoachEvent::ThrottleTooEarly { corner_id: corner.id },
                        format!(
                            "Corner {}: throttle is {}m too early — wait for the car to settle, TC fired {} times.",
                            corner.id, round_m(-delta_m), tc_hits
                        ),
                        3,
                        Some(corner.id),
                    ));
                }
            }
        }

        // ── Apex speed — only when no root-cause tip already explains it ─────
        if !brake_or_throttle_tip_fired && apex_delta > APEX_SPEED_THRESHOLD_KPH {
            tips.push(StructuredTip::new(
                CoachEvent::SlowApex {
                    corner_id: corner.id,
                    delta_kph: apex_delta,
                },
                format!(
                    "Corner {}: you're {:.0} kph slower at the apex than your reference.",
                    corner.id, apex_delta
                ),
                4,
                Some(corner.id),
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
