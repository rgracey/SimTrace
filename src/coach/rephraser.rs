//! The `Rephraser` trait abstracts the natural-language variation layer.
//!
//! Stage 1: `PassthroughRephraser` returns the fact string unchanged.
//! Stage 2: An `LlmRephraser` implementation will use the local LLM to vary
//!          phrasing while keeping the factual content intact.

use super::events::StructuredTip;

/// Converts a `StructuredTip` into the final spoken/displayed string.
pub trait Rephraser: Send + 'static {
    fn rephrase(&self, tip: &StructuredTip) -> String;
    /// Returns `true` when this rephraser is backed by a live LLM.
    fn is_llm(&self) -> bool {
        false
    }
}

/// Passthrough — returns `tip.fact` unchanged.
///
/// Used in Stage 1 (no LLM) and as a fallback when the LLM is unavailable.
pub struct PassthroughRephraser;

impl Rephraser for PassthroughRephraser {
    fn rephrase(&self, tip: &StructuredTip) -> String {
        tip.fact.clone()
    }
}
