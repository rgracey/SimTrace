//! AI Coach module.
//!
//! Public surface:
//! - [`CoachHandle`] — spawn the background thread and receive tips.
//! - [`CoachTip`] — a tip ready for display / TTS.
//! - [`CoachStatus`] — current state reported to the UI.

pub mod analyzer;
pub mod centerline;
pub mod corner;
pub mod downloader;
pub mod events;
pub mod lap;
pub mod llm;
pub mod reference;
pub mod rephraser;
pub mod track_map;
pub mod tts;

#[allow(unused_imports)]
pub use centerline::Centerline;
#[allow(unused_imports)]
pub use corner::{CornerDetector, DetectedCorner};
#[allow(unused_imports)]
pub use events::{CoachEvent, CoachTip, StructuredTip};
#[allow(unused_imports)]
pub use lap::{LapData, LapRecorder, LapSample};
#[allow(unused_imports)]
pub use reference::{CornerPerf, ReferenceLap, ReferenceMeta, ReferenceSource};
pub use track_map::TrackMap;
#[allow(unused_imports)]
pub use tts::{SilentSpeaker, Speaker};

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use tracing::info;

use crate::config::CoachConfig;
use crate::core::TelemetryBuffer;

// ── Public status ─────────────────────────────────────────────────────────────

/// Snapshot of coach state, sent to the UI alongside tips.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct CoachStatus {
    /// How many laps worth of data has been collected this session.
    pub laps_recorded: u32,
    /// Whether a track map has been built for the current circuit.
    pub has_track_map: bool,
    /// Number of confirmed corners in the track map.
    pub corner_count: usize,
    /// Whether a reference lap is loaded for comparison.
    pub has_reference: bool,
    /// Best self-recorded lap time this session (ms), if any.
    pub best_lap_ms: Option<u32>,
    /// Whether a TTS speaker is active and ready.
    pub tts_active: bool,
    /// Error string if TTS failed to initialize, or `None` when OK / not attempted.
    pub tts_error: Option<String>,
    /// How many laps have been averaged into the centerline (0 = none).
    pub centerline_laps: u32,
    /// Whether a centerline has been built for the current track.
    pub has_centerline: bool,
    /// Snapshot of the centerline points for the map renderer (empty when unavailable).
    pub centerline_points: Vec<centerline::CenterlinePoint>,
    /// Snapshot of the detected corners for the map renderer (empty when unavailable).
    pub map_corners: Vec<corner::DetectedCorner>,
    /// Track length for the map renderer (metres).
    pub map_track_length_m: f32,
}

// ── Internal messages ─────────────────────────────────────────────────────────

enum CoachMsg {
    Tip(CoachTip),
    Status(CoachStatus),
}

// ── CoachHandle ───────────────────────────────────────────────────────────────

/// Handle to the background coaching thread.
///
/// Dropping this value signals the thread to stop and joins it, ensuring the
/// LLM's Metal/CUDA backend is fully released before the caller continues.
pub struct CoachHandle {
    rx: mpsc::Receiver<CoachMsg>,
    shutdown: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for CoachHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl CoachHandle {
    /// Spawn the coach thread. Call this when `CoachConfig::enabled` is true.
    pub fn spawn(config: CoachConfig, buffer: Arc<TelemetryBuffer>) -> Self {
        let (tx, rx) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = Arc::clone(&shutdown);
        let thread = std::thread::Builder::new()
            .name("simtrace-coach".into())
            .spawn(move || coach_loop(config, buffer, tx, shutdown_clone))
            .expect("failed to spawn coach thread");
        Self {
            rx,
            shutdown,
            thread: Some(thread),
        }
    }

    /// Drain all pending tips and status updates. Call once per UI frame.
    pub fn drain(&self) -> (Vec<CoachTip>, Option<CoachStatus>) {
        let mut tips = Vec::new();
        let mut status = None;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                CoachMsg::Tip(t) => tips.push(t),
                CoachMsg::Status(s) => status = Some(s),
            }
        }
        (tips, status)
    }
}

// ── Background thread ─────────────────────────────────────────────────────────

/// Build the best available speaker for the current configuration.
///
/// Returns `(speaker, error)`.  When TTS is disabled or unavailable `speaker`
/// is `None`; `error` carries a human-readable reason when something went wrong.
fn build_speaker(_config: &CoachConfig) -> (Option<Box<dyn tts::Speaker>>, Option<String>) {
    if !_config.tts_enabled {
        return (None, None);
    }
    #[cfg(feature = "coach-tts")]
    match tts::NativeSpeaker::spawn() {
        Ok(s) => return (Some(Box::new(s)), None),
        Err(e) => return (None, Some(e.to_string())),
    }
    #[allow(unreachable_code)]
    (None, Some("build without --features coach-tts".into()))
}

fn coach_loop(
    config: CoachConfig,
    buffer: Arc<TelemetryBuffer>,
    tx: mpsc::Sender<CoachMsg>,
    shutdown: Arc<AtomicBool>,
) {
    let data_dir = config.data_dir();
    let tracks_dir = data_dir.join("tracks");
    let refs_dir = data_dir.join("references");

    let (speaker, tts_error) = build_speaker(&config);
    let tts_active = speaker.is_some();
    let mut lap_recorder = LapRecorder::new();
    let mut analyzer = analyzer::Analyzer::new();

    let mut centerline: Option<centerline::Centerline> = None;
    let mut track_map: Option<TrackMap> = None;
    let mut reference_lap: Option<ReferenceLap> = None;
    let mut best_lap_ms: Option<u32> = None;

    // Corner-tracking state: which corner are we inside right now.
    let mut active_corner_id: Option<u8> = None;
    let mut corner_samples: Vec<LapSample> = Vec::new();

    // Only process points we haven't seen before.
    let mut last_seen: Option<Instant> = None;

    let cooldown = Duration::from_secs(config.cooldown_secs as u64);
    let mut last_tip_at: Option<Instant> = None;
    // Separate cooldown for real-time global tips (TC/ABS/overlap/coast) so
    // they can't starve corner-specific tips of their cooldown slot.
    let realtime_cooldown = cooldown.max(Duration::from_secs(30));
    let mut last_realtime_tip_at: Option<Instant> = None;

    // Coach mode: pending tips to fire before each corner next lap.
    // Maps corner_id → (tip text, priority) so we can surface the worst corner in lap summaries.
    let mut pending_corner_tips: HashMap<u8, (String, u8)> = HashMap::new();
    // Track which corners have had their anticipatory tip fired this approach
    // (maps corner_id → track_pos when fired, to detect a new lap's approach).
    let mut anticipatory_fired: HashMap<u8, f32> = HashMap::new();
    // Per-corner apex speed from the most recent lap, for improvement detection.
    let mut last_apex_speed: HashMap<u8, f32> = HashMap::new();
    // Consecutive laps a corner has had the same highest-priority tip with no improvement.
    // Used for escalation (≥3 laps) and suppression (≥5 laps).
    let mut corner_tip_laps: HashMap<u8, (String, u32)> = HashMap::new(); // corner_id → (tip_text, streak)

    // Emit status every N seconds.
    let mut last_status_at = Instant::now();
    const STATUS_INTERVAL: Duration = Duration::from_secs(2);

    loop {
        if shutdown.load(Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50)); // 20 Hz

        // ── Collect new telemetry points ─────────────────────────────────────
        let all_points = buffer.get_points();
        let session = buffer.latest_session();

        let new_points: Vec<_> = all_points
            .iter()
            .filter(|p| last_seen.map_or(true, |t| p.captured_at > t))
            .collect();

        if let Some(p) = all_points.last() {
            last_seen = Some(p.captured_at);
        }

        // Try loading a saved track map and centerline for this session if we don't have one.
        if track_map.is_none() {
            if let Some(ref s) = session {
                if !s.track_name.is_empty() {
                    if let Some(map) = TrackMap::load(&tracks_dir, &s.track_name, s.track_length) {
                        info!(
                            "Coach: loaded track map '{}' ({} corners)",
                            map.track_name,
                            map.corners.len()
                        );
                        // Also try to load a saved centerline.
                        if centerline.is_none() {
                            centerline = centerline::Centerline::load(&tracks_dir, &map.file_stem());
                            if centerline.is_some() {
                                info!("Coach: loaded centerline");
                            }
                        }
                        // Also try to load a saved reference lap.
                        reference_lap =
                            ReferenceLap::load_self(&refs_dir, &map.file_stem(), &s.car_name);
                        if reference_lap.is_some() {
                            info!("Coach: loaded self reference lap");
                        }
                        track_map = Some(map);
                    }
                }
            }
        }

        for point in &new_points {
            let sample = LapSample::from_point(
                point,
                lap_recorder
                    .current_samples()
                    .first()
                    .map(|_| Instant::now()) // lap_start is internal; use relative elapsed
                    .unwrap_or(Instant::now()),
            );

            // ── Lap recording ─────────────────────────────────────────────
            let completed = lap_recorder.push(point, session.as_ref());

            // ── Real-time analysis ────────────────────────────────────────
            let rt_tips = analyzer.analyze_realtime(&sample);
            for tip in rt_tips {
                maybe_send_tip(
                    speaker.as_deref(),
                    tip,
                    &tx,
                    &mut last_realtime_tip_at,
                    realtime_cooldown,
                );
            }

            // ── Anticipatory tips ────────────────────────────────────────
            // Only the highest-priority pending corner fires per lap —
            // focus the driver on one thing at a time.
            if let Some(map) = &track_map {
                let track_len = if map.track_length_m > 0.0 {
                    map.track_length_m
                } else {
                    3000.0
                };

                // Find the single corner with the highest priority tip that still has
                // something to say.
                let focus_corner_id = pending_corner_tips
                    .iter()
                    .max_by_key(|(_, (_, pri))| *pri)
                    .map(|(&id, _)| id);

                if let Some(focus_id) = focus_corner_id {
                    if let Some(corner) = map.corner_by_id(focus_id) {
                        // Use the reference lap's brake point if available;
                        // fall back to the geometric turn-in otherwise.
                        let ref_brake = reference_lap
                            .as_ref()
                            .and_then(|r| r.corner(corner.id))
                            .map(|p| p.brake_point)
                            .unwrap_or(corner.turn_in);
                        let d_frac = {
                            let d = ref_brake - sample.track_pos;
                            if d < 0.0 {
                                d + 1.0
                            } else {
                                d
                            }
                        };
                        let dist_m = d_frac * track_len;
                        if dist_m > 50.0 && dist_m < 150.0 {
                            let already = anticipatory_fired
                                .get(&corner.id)
                                .map(|&fired_pos| {
                                    let gap = {
                                        let d = sample.track_pos - fired_pos;
                                        if d < 0.0 {
                                            d + 1.0
                                        } else {
                                            d
                                        }
                                    };
                                    gap < 0.8
                                })
                                .unwrap_or(false);
                            if !already {
                                if let Some((text, _)) =
                                    pending_corner_tips.get(&corner.id).cloned()
                                {
                                    send_tip_text(
                                        text,
                                        Some(corner.id),
                                        speaker.as_deref(),
                                        &tx,
                                        &mut last_tip_at,
                                        cooldown,
                                    );
                                    anticipatory_fired.insert(corner.id, sample.track_pos);
                                }
                            }
                        }
                    }
                }
            }

            // ── Corner tracking ───────────────────────────────────────────
            if let Some(map) = &track_map {
                let here = map.corner_at(sample.track_pos).map(|c| c.id);

                match (active_corner_id, here) {
                    (None, Some(id)) => {
                        // Entered a corner.
                        active_corner_id = Some(id);
                        corner_samples.clear();
                        corner_samples.push(sample);
                    }
                    (Some(prev), Some(id)) if prev == id => {
                        corner_samples.push(sample);
                    }
                    (Some(prev), _) => {
                        // Exited a corner — run post-corner analysis.
                        let ref_perf = reference_lap.as_ref().and_then(|r| r.corner(prev));
                        if let Some(c) = map.corner_by_id(prev).cloned() {
                            let track_len = track_map
                                .as_ref()
                                .map(|m| m.track_length_m)
                                .unwrap_or(3000.0);

                            // Compute apex speed for this corner exit.
                            let this_apex = corner_samples
                                .iter()
                                .map(|s| s.speed_kph)
                                .fold(f32::INFINITY, f32::min);
                            let this_apex = if this_apex.is_finite() {
                                Some(this_apex)
                            } else {
                                None
                            };

                            // Check for improvement vs last recorded apex speed.
                            const IMPROVEMENT_THRESHOLD_KPH: f32 = 3.0;
                            let improved = if let (Some(prev_apex), Some(curr_apex)) =
                                (last_apex_speed.get(&prev).copied(), this_apex)
                            {
                                curr_apex - prev_apex >= IMPROVEMENT_THRESHOLD_KPH
                            } else {
                                false
                            };

                            // Update stored apex speed for next lap's comparison.
                            if let Some(apex) = this_apex {
                                last_apex_speed.insert(prev, apex);
                            }

                            // Require >=2 laps of data before coaching this corner —
                            // by lap 2 the track map has been refined once and a
                            // reference lap exists, so tips are reliable enough.
                            let tips = if c.confidence >= 2 {
                                analyzer.analyze_corner(&c, &corner_samples, ref_perf, track_len)
                            } else {
                                vec![]
                            };

                            // Fire a positive acknowledgment when apex speed clearly improved.
                            if improved {
                                send_tip_text(
                                    format!("Better at turn {}.", prev),
                                    Some(prev),
                                    speaker.as_deref(),
                                    &tx,
                                    &mut last_tip_at,
                                    cooldown,
                                );
                            }

                            // Store the highest-priority tip for anticipatory delivery before the next lap.
                            // On the first time we see a corner, also send it immediately so lap 1
                            // isn't completely silent.
                            if let Some(best) = tips.into_iter().max_by_key(|t| t.priority) {
                                let base_text = best.fact.clone();
                                let priority = best.priority;
                                let first_time = !pending_corner_tips.contains_key(&prev);

                                // Update repeat streak.
                                let streak = {
                                    let entry = corner_tip_laps
                                        .entry(prev)
                                        .or_insert((base_text.clone(), 0));
                                    if entry.0 == base_text {
                                        entry.1 += 1;
                                    } else {
                                        // Different tip — issue changed, reset streak.
                                        *entry = (base_text.clone(), 1);
                                    }
                                    entry.1
                                };

                                if streak >= 5 {
                                    // Silenced — driver has heard this many times, drop it.
                                    pending_corner_tips.remove(&prev);
                                } else {
                                    // Escalate after 3 laps with the same tip and no improvement.
                                    let text = if streak >= 3 && !improved {
                                        format!("Still — {}. Commit to it.", base_text.trim_end_matches('.'))
                                    } else {
                                        base_text
                                    };
                                    pending_corner_tips.insert(prev, (text.clone(), priority));
                                    if first_time {
                                        send_tip_text(
                                            text,
                                            Some(prev),
                                            speaker.as_deref(),
                                            &tx,
                                            &mut last_tip_at,
                                            cooldown,
                                        );
                                    }
                                }
                            } else {
                                // No tips this exit — corner is clean.
                                corner_tip_laps.remove(&prev);
                                if improved {
                                    // Corner is clean and improved — remove from pending.
                                    pending_corner_tips.remove(&prev);
                                }
                            }
                        }
                        // Start the new corner if we immediately entered one.
                        active_corner_id = here;
                        corner_samples.clear();
                        if here.is_some() {
                            corner_samples.push(sample);
                        }
                    }
                    (None, None) => {}
                }
            }

            // ── On lap completion ─────────────────────────────────────────
            if let Some(lap) = completed {
                info!(
                    "Coach: lap {} complete — {} samples, time {:?}",
                    lap.lap_number,
                    lap.samples.len(),
                    lap.lap_time_ms
                );

                // Update best lap tracking.
                if let Some(t) = lap.lap_time_ms {
                    best_lap_ms = Some(match best_lap_ms {
                        Some(prev) => prev.min(t),
                        None => t,
                    });
                }

                // Build or refine centerline and track map.
                //
                // We need enough samples to cover a nearly-complete lap.
                // At 60 Hz a 30-second partial lap start yields ~1 800 samples,
                // so require at least 2 000 to rule those out.
                const MIN_DETECTION_SAMPLES: usize = 2000;
                let current_corner_count =
                    track_map.as_ref().map(|m| m.corners.len()).unwrap_or(0);

                if lap.samples.len() >= MIN_DETECTION_SAMPLES {
                    // Update centerline (build or blend).
                    if centerline.is_none() {
                        if let Some(cl) =
                            centerline::Centerline::from_lap(&lap.track_name, &lap.samples)
                        {
                            info!("Coach: built centerline from lap {}", lap.lap_number);
                            centerline = Some(cl);
                        }
                    } else if let Some(ref mut cl) = centerline {
                        cl.blend_lap(&lap.samples);
                        info!(
                            "Coach: blended lap {} into centerline ({} laps)",
                            lap.lap_number, cl.laps_averaged
                        );
                    }

                    // Build or refine track map.
                    let want_detect = track_map.is_none() || current_corner_count < 4;
                    if want_detect {
                        let corners =
                            CornerDetector::detect(centerline.as_ref(), &lap.samples);
                        if corners.len() > current_corner_count {
                            info!(
                                "Coach: detected {} corners on '{}' ({} samples)",
                                corners.len(),
                                lap.track_name,
                                lap.samples.len(),
                            );
                            // Corner IDs are changing — flush stale per-corner state.
                            if current_corner_count > 0 {
                                pending_corner_tips.clear();
                                anticipatory_fired.clear();
                                last_apex_speed.clear();
                                corner_tip_laps.clear();
                            }
                            let map = TrackMap::new(
                                lap.track_name.clone(),
                                lap.track_length_m,
                                corners,
                            );
                            if let Some(ref cl) = centerline {
                                let _ = cl.save(&tracks_dir, &map.file_stem());
                            }
                            let _ = map.save(&tracks_dir);
                            track_map = Some(map);
                        }
                    } else if let Some(ref mut map) = track_map {
                        for corner in map.corners.iter_mut() {
                            CornerDetector::refine(corner, &lap.samples);
                        }
                        if let Some(ref cl) = centerline {
                            let _ = cl.save(&tracks_dir, &map.file_stem());
                        }
                        let _ = map.save(&tracks_dir);
                    }
                }

                // ── Lap summary tip ───────────────────────────────────────
                if let Some(lap_time) = lap.lap_time_ms {
                    let lap_str = format_lap_time(lap_time);
                    let delta_str = reference_lap
                        .as_ref()
                        .and_then(|r| r.lap_time_ms)
                        .map(|ref_ms| {
                            let delta_s = (lap_time as f32 - ref_ms as f32) / 1000.0;
                            format!(" — {:+.1}s", delta_s)
                        })
                        .unwrap_or_default();
                    let focus_str = pending_corner_tips
                        .iter()
                        .max_by_key(|(_, (_, pri))| *pri)
                        .map(|(&id, _)| format!(" Focus turn {}.", id))
                        .unwrap_or_default();
                    let summary = format!("{}{}.{}", lap_str, delta_str, focus_str);
                    send_tip_text(
                        summary,
                        None,
                        speaker.as_deref(),
                        &tx,
                        &mut last_tip_at,
                        cooldown,
                    );
                }

                // Update reference lap according to the configured strategy.
                if let Some(ref map) = track_map {
                    let new_ref = ReferenceLap::from_lap(&lap, map, "");
                    let should_replace = match &reference_lap {
                        None => true,
                        Some(existing) => {
                            use crate::config::ReferenceLapStrategy;
                            match config.reference_lap_strategy {
                                ReferenceLapStrategy::Best => new_ref.is_better_than(existing),
                                ReferenceLapStrategy::Last => true,
                            }
                        }
                    };
                    if should_replace {
                        info!("Coach: updating reference lap");
                        let _ = new_ref.save(&refs_dir, &map.file_stem());
                        reference_lap = Some(new_ref);
                    }
                }
            }
        }

        // ── Periodic status update ────────────────────────────────────────────
        if last_status_at.elapsed() >= STATUS_INTERVAL {
            last_status_at = Instant::now();
            let status = CoachStatus {
                laps_recorded: lap_recorder.current_samples().len() as u32, // proxy
                has_track_map: track_map.is_some(),
                corner_count: track_map.as_ref().map(|m| m.corners.len()).unwrap_or(0),
                has_reference: reference_lap.is_some(),
                best_lap_ms,
                tts_active,
                tts_error: tts_error.clone(),
                centerline_laps: centerline.as_ref().map(|c| c.laps_averaged).unwrap_or(0),
                has_centerline: centerline.is_some(),
                centerline_points: centerline
                    .as_ref()
                    .map(|c| c.points.clone())
                    .unwrap_or_default(),
                map_corners: track_map
                    .as_ref()
                    .map(|m| m.corners.clone())
                    .unwrap_or_default(),
                map_track_length_m: track_map
                    .as_ref()
                    .map(|m| m.track_length_m)
                    .unwrap_or(0.0),
            };
            // Best-effort send — if the UI thread is gone, we'll exit next iteration.
            if tx.send(CoachMsg::Status(status)).is_err() {
                return;
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn format_lap_time(ms: u32) -> String {
    let mins = ms / 60_000;
    let secs = (ms % 60_000) as f32 / 1000.0;
    format!("{}:{:06.3}", mins, secs)
}

/// Send a pre-rephrased tip text directly (used in Coach/anticipatory mode).
fn send_tip_text(
    text: String,
    corner_id: Option<u8>,
    speaker: Option<&dyn tts::Speaker>,
    tx: &mpsc::Sender<CoachMsg>,
    last_tip_at: &mut Option<Instant>,
    cooldown: Duration,
) {
    let ready = last_tip_at.map_or(true, |t| t.elapsed() >= cooldown);
    if !ready {
        return;
    }
    if let Some(s) = speaker {
        s.speak(&text);
    }
    let coach_tip = CoachTip {
        text,
        corner_id,
        priority: 3,
        generated_at: Instant::now(),
    };
    if tx.send(CoachMsg::Tip(coach_tip)).is_ok() {
        *last_tip_at = Some(Instant::now());
    }
}

fn maybe_send_tip(
    speaker: Option<&dyn tts::Speaker>,
    tip: StructuredTip,
    tx: &mpsc::Sender<CoachMsg>,
    last_tip_at: &mut Option<Instant>,
    cooldown: Duration,
) {
    let ready = last_tip_at.map_or(true, |t| t.elapsed() >= cooldown);
    if !ready {
        return;
    }
    let text = tip.fact.clone();
    if let Some(s) = speaker {
        s.speak(&text);
    }
    let coach_tip = CoachTip {
        text,
        corner_id: tip.corner_id,
        priority: tip.priority,
        generated_at: Instant::now(),
    };
    if tx.send(CoachMsg::Tip(coach_tip)).is_ok() {
        *last_tip_at = Some(Instant::now());
    }
}
