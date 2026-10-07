//! Optional model providers beyond the Codex CLI adapter.
//!
//! Owns HTTP/local backends such as Ollama and Apple MLX LM. Codex process
//! spawning stays in `crate::codex`. Role routing and authority remain in `app`.

pub mod json_hydrate;
pub mod mlx_lm;
pub mod ollama;
pub mod openai_compat;
