//! AI Coach module.
//!
//! Public surface:
//! - [`CoachHandle`] — spawn the background thread and receive tips.
//! - [`CoachTip`] — a tip ready for display / TTS.
//! - [`CoachStatus`] — current state reported to the UI.

pub mod analyzer;
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
pub use events::{CoachEvent, CoachTip, StructuredTip};
#[allow(unused_imports)]
pub use lap::{LapData, LapRecorder, LapSample};
#[allow(unused_imports)]
pub use corner::{CornerDetector, DetectedCorner};
pub use track_map::TrackMap;
#[allow(unused_imports)]
pub use reference::{CornerPerf, ReferenceLap, ReferenceMeta, ReferenceSource};
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
        Self { rx, shutdown, thread: Some(thread) }
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

fn coach_loop(config: CoachConfig, buffer: Arc<TelemetryBuffer>, tx: mpsc::Sender<CoachMsg>, shutdown: Arc<AtomicBool>) {
    let data_dir = config.data_dir();
    let tracks_dir = data_dir.join("tracks");
    let refs_dir = data_dir.join("references");

    let (speaker, tts_error) = build_speaker(&config);
    let tts_active = speaker.is_some();
    let mut lap_recorder = LapRecorder::new();
    let mut analyzer = analyzer::Analyzer::new();

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

    // Coach mode: pending tips to fire before each corner next lap.
    // Maps corner_id → tip text.
    let mut pending_corner_tips: HashMap<u8, String> = HashMap::new();
    // Track which corners have had their anticipatory tip fired this approach
    // (maps corner_id → track_pos when fired, to detect a new lap's approach).
    let mut anticipatory_fired: HashMap<u8, f32> = HashMap::new();

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

        // Try loading a saved track map for this session if we don't have one.
        if track_map.is_none() {
            if let Some(ref s) = session {
                if !s.track_name.is_empty() {
                    if let Some(map) =
                        TrackMap::load(&tracks_dir, &s.track_name, s.track_length)
                    {
                        info!(
                            "Coach: loaded track map '{}' ({} corners)",
                            map.track_name,
                            map.corners.len()
                        );
                        // Also try to load a saved reference lap.
                        reference_lap = ReferenceLap::load_self(
                            &refs_dir,
                            &map.file_stem(),
                            &s.car_name,
                        );
                        if reference_lap.is_some() {
                            info!("Coach: loaded self reference lap");
                        }
                        track_map = Some(map);
                    }
                }
            }
        }

        for point in &new_points {
            let sample = LapSample::from_point(point, lap_recorder.current_samples()
                .first()
                .map(|_| Instant::now()) // lap_start is internal; use relative elapsed
                .unwrap_or(Instant::now()));

            // ── Lap recording ─────────────────────────────────────────────
            let completed = lap_recorder.push(point, session.as_ref());

            // ── Real-time analysis ────────────────────────────────────────
            let rt_tips = analyzer.analyze_realtime(&sample);
            for tip in rt_tips {
                maybe_send_tip(
                    speaker.as_deref(),
                    tip,
                    &tx,
                    &mut last_tip_at,
                    cooldown,
                );
            }

            // ── Anticipatory tips ────────────────────────────────────────
            // Tips fire before corners on the next lap.
            if let Some(map) = &track_map {
                let track_len = if map.track_length_m > 0.0 { map.track_length_m } else { 3000.0 };
                for corner in &map.corners {
                    let d_frac = {
                        let d = corner.brake_point - sample.track_pos;
                        if d < 0.0 { d + 1.0 } else { d }
                    };
                    let dist_m = d_frac * track_len;
                    if dist_m > 50.0 && dist_m < 150.0 {
                        let already = anticipatory_fired.get(&corner.id)
                            .map(|&fired_pos| {
                                let gap = {
                                    let d = sample.track_pos - fired_pos;
                                    if d < 0.0 { d + 1.0 } else { d }
                                };
                                gap < 0.8
                            })
                            .unwrap_or(false);
                        if !already {
                            if let Some(text) = pending_corner_tips.get(&corner.id).cloned() {
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
                            let track_len = track_map.as_ref().map(|m| m.track_length_m).unwrap_or(3000.0);
                            let tips =
                                analyzer.analyze_corner(&c, &corner_samples, ref_perf, track_len);
                            // Store the highest-priority tip for anticipatory delivery before the next lap.
                            // On the first time we see a corner, also send it immediately so lap 1
                            // isn't completely silent.
                            if let Some(best) = tips.into_iter().max_by_key(|t| t.priority) {
                                let text = best.fact.clone();
                                let first_time = !pending_corner_tips.contains_key(&prev);
                                pending_corner_tips.insert(prev, text.clone());
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

                // Build or refine track map.
                if track_map.is_none() && lap.samples.len() > 100 {
                    let corners = CornerDetector::detect(&lap.samples);
                    if !corners.is_empty() {
                        let map = TrackMap::new(
                            lap.track_name.clone(),
                            lap.track_length_m,
                            corners,
                        );
                        info!(
                            "Coach: detected {} corners on '{}'",
                            map.corners.len(),
                            map.track_name
                        );
                        let _ = map.save(&tracks_dir);
                        track_map = Some(map);
                    }
                } else if let Some(ref mut map) = track_map {
                    for corner in map.corners.iter_mut() {
                        CornerDetector::refine(corner, &lap.samples);
                    }
                    let _ = map.save(&tracks_dir);
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
            };
            // Best-effort send — if the UI thread is gone, we'll exit next iteration.
            if tx.send(CoachMsg::Status(status)).is_err() {
                return;
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

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
