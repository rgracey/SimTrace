//! Local LLM rephraser — wraps llama.cpp via the `llama-cpp-2` crate.
//!
//! Compiled only when the `coach-llm` Cargo feature is enabled.
//! The model (Qwen2.5-0.5B-Instruct Q8_0) is loaded once at coach startup
//! and kept resident in memory.  Each `rephrase()` call creates a short-lived
//! `LlamaContext`, runs greedy decoding, then drops the context.
//!
//! If inference fails for any reason the fact string is returned unchanged
//! (same behaviour as `PassthroughRephraser`).

#[cfg(feature = "coach-llm")]
use std::num::NonZeroU32;
#[cfg(feature = "coach-llm")]
use std::path::Path;
#[cfg(feature = "coach-llm")]
use super::events::StructuredTip;
#[cfg(feature = "coach-llm")]
use super::rephraser::Rephraser;

// ── Prompt ────────────────────────────────────────────────────────────────────

#[cfg(feature = "coach-llm")]
const SYSTEM_PROMPT: &str = "You are an AI sim-racing coach. \
Rephrase the following coaching tip to sound natural and conversational. \
Keep all numeric facts and technical advice exactly correct. \
Reply with ONLY the rephrased tip — no preamble, no quotes, no explanation. \
Maximum two short sentences.";

#[cfg(feature = "coach-llm")]
fn build_prompt(fact: &str) -> String {
    format!(
        "<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n\
         <|im_start|>user\n{fact}<|im_end|>\n\
         <|im_start|>assistant\n"
    )
}

// ── LlmRephraser ─────────────────────────────────────────────────────────────

#[cfg(feature = "coach-llm")]
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel},
    sampling::LlamaSampler,
};

/// Rephraser backed by a locally-running Qwen2.5 GGUF model.
#[cfg(feature = "coach-llm")]
pub struct LlmRephraser {
    // Drop order matters: model must be dropped before backend.
    // Rust drops fields in declaration order, so model is declared first.
    model: LlamaModel,
    _backend: LlamaBackend,
    n_ctx: NonZeroU32,
    max_new_tokens: usize,
    temperature: f32,
}

#[cfg(feature = "coach-llm")]
impl LlmRephraser {
    /// Load the GGUF model from disk.  Blocks for 1–5 s on first load.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let backend = LlamaBackend::init()?;
        let model_params = LlamaModelParams::default();
        let model = LlamaModel::load_from_file(&backend, path, &model_params)?;
        Ok(Self {
            model,
            _backend: backend,
            n_ctx: NonZeroU32::new(512).unwrap(),
            max_new_tokens: 120,
            temperature: 0.7,
        })
    }

    fn run_inference(&self, fact: &str) -> anyhow::Result<String> {
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(self.n_ctx))
            .with_n_threads(4)
            .with_n_threads_batch(4);
        let mut ctx = self.model.new_context(&self._backend, ctx_params)?;

        let prompt = build_prompt(fact);
        let tokens = self.model.str_to_token(&prompt, AddBos::Always)?;
        let n_prompt = tokens.len();

        // Encode the prompt in one batch (logits only for the last token).
        let mut batch = LlamaBatch::new(n_prompt + self.max_new_tokens, 1);
        batch.add_sequence(&tokens, 0, false)?;
        ctx.decode(&mut batch)?;

        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::temp(self.temperature),
            LlamaSampler::greedy(),
        ]);

        let mut output = String::new();
        let mut n_cur = n_prompt as i32;

        for _ in 0..self.max_new_tokens {
            let token = sampler.sample(&ctx, n_cur - 1);
            if self.model.is_eog_token(token) {
                break;
            }
            let piece = self.model.token_to_piece(token, false)?;
            output.push_str(&piece);

            // Feed the new token back for the next step.
            batch.clear();
            batch.add(token, n_cur, &[0], true)?;
            ctx.decode(&mut batch)?;
            n_cur += 1;
        }

        Ok(output.trim().to_string())
    }
}

#[cfg(feature = "coach-llm")]
impl Rephraser for LlmRephraser {
    fn rephrase(&self, tip: &StructuredTip) -> String {
        match self.run_inference(&tip.fact) {
            Ok(text) if !text.is_empty() => text,
            _ => tip.fact.clone(),
        }
    }
}
