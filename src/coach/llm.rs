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
use super::events::StructuredTip;
#[cfg(feature = "coach-llm")]
use super::rephraser::Rephraser;
#[cfg(feature = "coach-llm")]
use std::num::NonZeroU32;
#[cfg(feature = "coach-llm")]
use std::path::Path;

// ── Prompt ────────────────────────────────────────────────────────────────────

#[cfg(feature = "coach-llm")]
const SYSTEM_PROMPT: &str = "Rewrite the input as a one-sentence race engineer radio call. \
Keep every number exactly as given. Reply with only the sentence — nothing else.";

/// Few-shot examples that show the model exactly what style is expected.
#[cfg(feature = "coach-llm")]
const EXAMPLES: &[(&str, &str)] = &[
    (
        "Corner 3: brake 30m later.",
        "Brake 30 metres later into turn 3.",
    ),
    (
        "Corner 5: get on the power 20m earlier on exit.",
        "Throttle up 20 metres earlier out of turn 5.",
    ),
    (
        "Corner 7: entry is 14 kph too fast — move the brake point 20m earlier.",
        "You're 14 kph too hot into turn 7 — move the brake point back 20 metres.",
    ),
    (
        "ABS has activated 6 times in 15 seconds — you're arriving at corners with too much brake pressure.",
        "Ease the brake pressure — ABS has fired 6 times in the last 15 seconds.",
    ),
    (
        "Corner 2: you're 9 kph slower at the apex than your reference.",
        "You're losing 9 kph at the apex of turn 2 — carry more speed through the middle.",
    ),
];

#[cfg(feature = "coach-llm")]
fn build_prompt(fact: &str) -> String {
    let mut prompt = format!("<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n");
    for (user, assistant) in EXAMPLES {
        prompt.push_str(&format!(
            "<|im_start|>user\n{user}<|im_end|>\n\
             <|im_start|>assistant\n{assistant}<|im_end|>\n"
        ));
    }
    prompt.push_str(&format!(
        "<|im_start|>user\n{fact}<|im_end|>\n\
         <|im_start|>assistant\n"
    ));
    prompt
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
            n_ctx: NonZeroU32::new(768).unwrap(),
            max_new_tokens: 60,
            temperature: 0.3,
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

        // Encode the prompt — request logits only for the last token.
        let mut batch = LlamaBatch::new(n_prompt + self.max_new_tokens, 1);
        for (i, &token) in tokens.iter().enumerate() {
            let is_last = i == tokens.len() - 1;
            batch.add(token, i as i32, &[0], is_last)?;
        }
        ctx.decode(&mut batch)?;

        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::temp(self.temperature),
            LlamaSampler::greedy(),
        ]);

        let mut output = String::new();
        let mut n_cur = n_prompt as i32;
        let mut decoder = encoding_rs::UTF_8.new_decoder();

        for _ in 0..self.max_new_tokens {
            let token = sampler.sample(&ctx, -1);
            if self.model.is_eog_token(token) {
                break;
            }
            let piece = self
                .model
                .token_to_piece(token, &mut decoder, false, None)?;
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
            Ok(text) if !text.is_empty() => {
                tracing::debug!("LLM rephrased: {text}");
                text
            }
            Ok(_) => {
                tracing::warn!("LLM returned empty output, using original fact");
                tip.fact.clone()
            }
            Err(e) => {
                tracing::warn!("LLM inference failed: {e}");
                tip.fact.clone()
            }
        }
    }

    fn is_llm(&self) -> bool {
        true
    }
}
