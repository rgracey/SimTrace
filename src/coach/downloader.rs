//! Background model downloader.
//!
//! Downloads the GGUF model file from HuggingFace, writing progress into a
//! shared `Arc<Mutex<DownloadState>>` so the UI can render a progress bar.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::fmt;

// ── Public URLs ───────────────────────────────────────────────────────────────

/// HuggingFace URL for the Qwen2.5-0.5B-Instruct Q8_0 GGUF model (~500 MB).
pub const DEFAULT_MODEL_URL: &str =
    "https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/\
     qwen2.5-0.5b-instruct-q8_0.gguf";


// ── DownloadState ─────────────────────────────────────────────────────────────

/// Current state of the LLM model file.
#[derive(Debug, Clone, PartialEq)]
pub enum DownloadState {
    /// Model file is absent.
    NotDownloaded,
    /// A download is in progress; value is 0.0–1.0 (indeterminate if 0.0).
    Downloading(f32),
    /// Model file is present and large enough to be valid.
    Ready,
    /// A download attempt failed.
    Failed(String),
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns `true` when the file exists and is larger than 1 MiB
/// (guards against partial / zero-byte files from a previous failed run).
pub fn model_exists(path: &Path) -> bool {
    path.exists() && path.metadata().map(|m| m.len() > 1024 * 1024).unwrap_or(false)
}


// ── Download ──────────────────────────────────────────────────────────────────

/// Spawns a background thread that downloads `url` to `dest_path`,
/// updating `state` as it progresses.
///
/// Drops the returned `JoinHandle` — the thread is detached and will
/// finish even if the caller does not join it.
pub fn start_download(url: String, dest_path: PathBuf, state: Arc<Mutex<DownloadState>>) {
    *state.lock().unwrap() = DownloadState::Downloading(0.0);
    std::thread::Builder::new()
        .name("model-downloader".into())
        .spawn(move || download_task(url, dest_path, state))
        .expect("failed to spawn model downloader thread");
}

fn download_task(url: String, dest_path: PathBuf, state: Arc<Mutex<DownloadState>>) {
    let result = do_download(&url, &dest_path, &state);
    *state.lock().unwrap() = match result {
        Ok(()) => DownloadState::Ready,
        Err(e) => {
            let _ = std::fs::remove_file(&dest_path.with_extension("gguf.tmp"));
            DownloadState::Failed(e.to_string())
        }
    };
}

fn do_download(
    url: &str,
    dest_path: &Path,
    state: &Arc<Mutex<DownloadState>>,
) -> anyhow::Result<()> {
    // Ensure the parent directory exists.
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let response = ureq::get(url)
        .call()
        .map_err(|e| anyhow::anyhow!("HTTP request failed: {e}"))?;

    let total_bytes = response
        .header("content-length")
        .and_then(|s| s.parse::<u64>().ok());

    // Write to a temp path; rename atomically on success.
    let tmp_path = dest_path.with_extension("gguf.tmp");
    let mut file = std::fs::File::create(&tmp_path)?;

    let mut reader = response.into_reader();
    let mut buf = [0u8; 65_536]; // 64 KiB chunks
    let mut downloaded: u64 = 0;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        downloaded += n as u64;

        let progress = total_bytes
            .filter(|&t| t > 0)
            .map(|t| (downloaded as f32 / t as f32).clamp(0.0, 1.0))
            .unwrap_or(0.0);

        *state.lock().unwrap() = DownloadState::Downloading(progress);
    }

    file.flush()?;
    drop(file);
    std::fs::rename(&tmp_path, dest_path)?;
    Ok(())
}


// ── Display for DownloadState ─────────────────────────────────────────────────

impl fmt::Display for DownloadState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DownloadState::NotDownloaded => write!(f, "not downloaded"),
            DownloadState::Downloading(p) => write!(f, "downloading ({:.0}%)", p * 100.0),
            DownloadState::Ready => write!(f, "ready"),
            DownloadState::Failed(e) => write!(f, "failed: {e}"),
        }
    }
}
