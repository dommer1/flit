//! Running the local model: one worker thread owns llama.cpp and answers
//! completion requests one at a time.
//!
//! why a dedicated std thread instead of spawn_blocking: llama.cpp's model
//! and context handles are not `Send`, and a loaded model is gigabytes we
//! want to keep warm between requests — a thread that owns them for its
//! whole life is the simplest way to have both. Requests arrive on a
//! channel, so callers never touch the handles; that also serialises
//! inference, which is what a single laptop GPU wants anyway.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::OnceLock;
use std::time::Duration;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::{send_logs_to_tracing, LogOptions};

use crate::error::AppError;

/// Tokens of context per request: prompt plus answer. The prompt builders
/// budget against this (see summarize.rs); the engine only enforces it.
pub const N_CTX: u32 = 8192;
/// Tokens fed per decode call while reading the prompt.
const N_BATCH: u32 = 512;
/// A loaded model is dropped after this long without a request, giving the
/// memory back — reading mail must not cost gigabytes of RAM all day.
const IDLE_UNLOAD: Duration = Duration::from_secs(5 * 60);
/// Sampling: near-deterministic, with a light repetition penalty — a
/// summary should be faithful, not creative.
const TEMPERATURE: f32 = 0.3;
const TOP_P: f32 = 0.9;
const REPEAT_LAST_N: i32 = 64;
const REPEAT_PENALTY: f32 = 1.1;

/// One turn of a chat prompt; `role` is "system", "user" or "assistant".
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
}

/// A completion request: which model file, the conversation so far, and
/// how many tokens the answer may take.
#[derive(Debug, Clone)]
pub struct Request {
    pub model_path: PathBuf,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: usize,
    /// Text placed after the assistant header, as if the model had already
    /// written it. Models with a "thinking" mode take an empty think block
    /// here (see catalog) so the answer starts right away instead of after
    /// hundreds of tokens of deliberation.
    pub assistant_prefix: String,
}

type Reply = tokio::sync::oneshot::Sender<Result<String, AppError>>;

/// Handle to the worker thread. Cheap to keep in AppState: the thread —
/// and llama.cpp's backend — start on the first request, not at app boot.
#[derive(Default)]
pub struct Engine {
    sender: OnceLock<mpsc::Sender<(Request, Reply)>>,
}

impl Engine {
    /// Run one completion; resolves when the answer is complete.
    pub async fn complete(&self, request: Request) -> Result<String, AppError> {
        let (reply, answer) = tokio::sync::oneshot::channel();
        self.sender()
            .send((request, reply))
            .map_err(|_| AppError::Llm("the model worker has stopped".to_string()))?;
        answer
            .await
            .map_err(|_| AppError::Llm("the model worker dropped the request".to_string()))?
    }

    fn sender(&self) -> &mpsc::Sender<(Request, Reply)> {
        self.sender.get_or_init(|| {
            let (sender, receiver) = mpsc::channel();
            // why ignore the spawn error: a thread that could not start
            // drops `receiver`, and every send then fails with a clear
            // "worker has stopped" instead of a panic here.
            let _ = std::thread::Builder::new()
                .name("llm".to_string())
                .spawn(move || worker(receiver));
            sender
        })
    }
}

/// The worker loop: load on demand, answer, unload when idle.
fn worker(receiver: mpsc::Receiver<(Request, Reply)>) {
    // llama.cpp logs every model detail to stderr by default — keep it quiet.
    send_logs_to_tracing(LogOptions::default().with_logs_enabled(false));
    let backend = match LlamaBackend::init() {
        Ok(backend) => backend,
        Err(err) => {
            // Fail every request with the reason rather than exit silently.
            while let Ok((_, reply)) = receiver.recv() {
                let _ = reply.send(Err(AppError::Llm(format!("cannot start llama.cpp: {err}"))));
            }
            return;
        }
    };
    let mut loaded: Option<Loaded> = None;
    loop {
        match receiver.recv_timeout(IDLE_UNLOAD) {
            Ok((request, reply)) => {
                let result = complete(&backend, &mut loaded, &request);
                let _ = reply.send(result);
            }
            Err(RecvTimeoutError::Timeout) => loaded = None,
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

struct Loaded {
    path: PathBuf,
    model: LlamaModel,
}

fn complete(
    backend: &LlamaBackend,
    loaded: &mut Option<Loaded>,
    request: &Request,
) -> Result<String, AppError> {
    let model = load(backend, loaded, &request.model_path)?;

    let template = model
        .chat_template(None)
        .map_err(|e| AppError::Llm(format!("model has no chat template: {e}")))?;
    let messages = request
        .messages
        .iter()
        .map(|m| LlamaChatMessage::new(m.role.to_string(), m.content.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::Llm(format!("bad prompt text: {e}")))?;
    let mut prompt = model
        .apply_chat_template(&template, &messages, true)
        .map_err(|e| AppError::Llm(format!("cannot apply chat template: {e}")))?;
    prompt.push_str(&request.assistant_prefix);
    // AddBos::Always means "if this model wants one" — llama.cpp consults
    // the vocabulary, so Gemma gets its <bos> and Qwen stays without.
    let tokens = model
        .str_to_token(&prompt, AddBos::Always)
        .map_err(|e| AppError::Llm(format!("cannot tokenize prompt: {e}")))?;
    if tokens.len() + request.max_tokens > N_CTX as usize {
        return Err(AppError::Llm(format!(
            "prompt of {} tokens does not fit the {N_CTX}-token context",
            tokens.len()
        )));
    }

    let params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(N_CTX))
        .with_n_batch(N_BATCH);
    let mut ctx = model
        .new_context(backend, params)
        .map_err(|e| AppError::Llm(format!("cannot create context: {e}")))?;

    // Feed the prompt in batches; logits are only needed for its last token.
    let mut batch = LlamaBatch::new(N_BATCH as usize, 1);
    let last = tokens.len() - 1;
    for (start, chunk) in tokens
        .chunks(N_BATCH as usize)
        .enumerate()
        .map(|(i, c)| (i * N_BATCH as usize, c))
    {
        batch.clear();
        for (offset, token) in chunk.iter().enumerate() {
            let pos = start + offset;
            batch
                .add(*token, pos as i32, &[0], pos == last)
                .map_err(|e| AppError::Llm(format!("cannot batch prompt: {e}")))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| AppError::Llm(format!("cannot read prompt: {e}")))?;
    }

    let mut sampler = LlamaSampler::chain_simple([
        LlamaSampler::penalties(model.n_vocab(), REPEAT_LAST_N, REPEAT_PENALTY, 0.0, 0.0),
        LlamaSampler::temp(TEMPERATURE),
        LlamaSampler::top_p(TOP_P, 1),
        LlamaSampler::dist(1234),
    ]);
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut output = String::new();
    for pos in (tokens.len()..).take(request.max_tokens) {
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
        sampler.accept(token);
        if model.is_eog_token(token) {
            break;
        }
        let piece = model
            .token_to_piece(token, &mut decoder, false, None)
            .map_err(|e| AppError::Llm(format!("cannot decode token: {e}")))?;
        output.push_str(&piece);

        batch.clear();
        batch
            .add(token, pos as i32, &[0], true)
            .map_err(|e| AppError::Llm(format!("cannot batch token: {e}")))?;
        ctx.decode(&mut batch)
            .map_err(|e| AppError::Llm(format!("generation failed: {e}")))?;
    }
    Ok(strip_thinking(&output).trim().to_string())
}

/// Drop a leading `<think>…</think>` block — a model that deliberated
/// despite the prefill must not have its notes shown as the summary. An
/// unterminated block means it ran out of tokens while thinking: nothing
/// usable was produced.
fn strip_thinking(output: &str) -> &str {
    let trimmed = output.trim_start();
    let Some(rest) = trimmed.strip_prefix("<think>") else {
        return output;
    };
    match rest.find("</think>") {
        Some(end) => &rest[end + "</think>".len()..],
        None => "",
    }
}

/// The model for `path`, loading it — and dropping the previous one first,
/// so two models never sit in memory together — when it is not the one
/// already loaded.
fn load<'a>(
    backend: &LlamaBackend,
    loaded: &'a mut Option<Loaded>,
    path: &PathBuf,
) -> Result<&'a LlamaModel, AppError> {
    if loaded.as_ref().is_none_or(|l| &l.path != path) {
        *loaded = None;
        // All layers on the GPU where there is one (Metal on Apple
        // Silicon); without a GPU backend the number is simply ignored.
        let params = LlamaModelParams::default().with_n_gpu_layers(1000);
        let model = LlamaModel::load_from_file(backend, path, &params)
            .map_err(|e| AppError::Llm(format!("cannot load model: {e}")))?;
        *loaded = Some(Loaded {
            path: path.clone(),
            model,
        });
    }
    Ok(&loaded.as_ref().expect("just loaded").model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_a_leading_think_block() {
        assert_eq!(
            strip_thinking("<think>\nhmm\n</think>\n\nThe answer.").trim(),
            "The answer."
        );
        assert_eq!(strip_thinking("The answer."), "The answer.");
        assert_eq!(strip_thinking("<think>\nstill thinking"), "");
        // Only a leading block is a thinking block; the tag mid-text stays.
        assert_eq!(strip_thinking("A <think> B"), "A <think> B");
    }

    /// Needs a real model file: `FLIT_TEST_MODEL=/path/to/model.gguf cargo
    /// test llm::engine -- --ignored`. Loads it and checks that a trivial
    /// instruction gets the expected answer.
    #[tokio::test]
    #[ignore]
    async fn answers_a_trivial_instruction() {
        let path = std::env::var("FLIT_TEST_MODEL").expect("FLIT_TEST_MODEL not set");
        let engine = Engine::default();

        let answer = engine
            .complete(Request {
                model_path: PathBuf::from(path),
                messages: vec![
                    ChatMessage {
                        role: "system",
                        content: "You answer with a single word.".to_string(),
                    },
                    ChatMessage {
                        role: "user",
                        content: "Say the word: pong".to_string(),
                    },
                ],
                max_tokens: 16,
                assistant_prefix: "<think>\n\n</think>\n\n".to_string(),
            })
            .await
            .unwrap();

        assert!(answer.to_lowercase().contains("pong"), "{answer:?}");
    }
}
