//! TTS (text-to-speech) abstraction backed by the platform's native speech engine.
//!
//! Uses the `tts` crate which delegates to:
//! - **macOS**: AVSpeechSynthesizer
//! - **Windows**: WinRT (Windows 11 neural voices) or SAPI fallback
//! - **Linux**: Speech Dispatcher
//!
//! No model files, no external binaries, no downloads required.
//! Compiled only when the `coach-tts` feature is enabled.

// ── Speaker trait ─────────────────────────────────────────────────────────────

/// Converts tip text to spoken audio output.
pub trait Speaker: Send + 'static {
    fn speak(&self, text: &str);
}

// ── SilentSpeaker ─────────────────────────────────────────────────────────────

/// No-op speaker used when TTS is disabled or unavailable.
#[allow(dead_code)]
pub struct SilentSpeaker;

impl Speaker for SilentSpeaker {
    fn speak(&self, _text: &str) {}
}

// ── NativeSpeaker ─────────────────────────────────────────────────────────────

/// Native OS TTS speaker.
///
/// The `tts::Tts` engine is not `Send`, so it lives on a dedicated thread.
/// `NativeSpeaker` itself is `Send` — it only holds a channel sender.
#[cfg(feature = "coach-tts")]
pub struct NativeSpeaker {
    tx: std::sync::mpsc::Sender<String>,
    _thread: std::thread::JoinHandle<()>,
}

#[cfg(feature = "coach-tts")]
impl NativeSpeaker {
    /// Initialise the native TTS engine and return a handle.
    ///
    /// Returns `Err` only if the thread cannot be spawned.  Engine init
    /// happens on the thread itself; errors are logged but won't propagate
    /// here (the thread simply exits, and `speak()` calls become no-ops).
    pub fn spawn() -> anyhow::Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let thread = std::thread::Builder::new()
            .name("simtrace-tts".into())
            .spawn(move || tts_thread(rx))?;
        Ok(Self {
            tx,
            _thread: thread,
        })
    }
}

#[cfg(feature = "coach-tts")]
impl Speaker for NativeSpeaker {
    fn speak(&self, text: &str) {
        // Blocking send — the TTS thread always drains the channel promptly,
        // and the 20 s coach cooldown means this queue rarely has more than 1 item.
        let _ = self.tx.send(text.to_string());
    }
}

// ── TTS thread ────────────────────────────────────────────────────────────────

#[cfg(feature = "coach-tts")]
fn tts_thread(rx: std::sync::mpsc::Receiver<String>) {
    let mut engine = match tts::Tts::default() {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("TTS: failed to initialise native engine — {e}");
            return;
        }
    };
    tracing::info!("TTS: native engine ready");

    for text in rx {
        // `interrupt = false` — let each utterance play fully.
        // Tips are spaced by cooldown so they won't pile up.
        if let Err(e) = engine.speak(&text, false) {
            tracing::warn!("TTS: speak error — {e}");
        }
    }
}
