//! Prompt and schema assets.
//!
//! Owns loading role prompt text and JSON schemas from repository data files via
//! `include_str!`. This keeps large prose/schema payloads out of orchestration
//! source so agents can edit logic without reading every role prompt.
//!
//! Read when changing prompt/schema file paths or adding a new role asset. Do
//! not read when changing role routing, verification, or queue parsing.

pub(crate) const DEFAULT_AGENTS_MD: &str = include_str!("../prompts/agents.md");
pub(crate) const MASTER_PROMPT: &str = include_str!("../prompts/master.md");
pub(crate) const PLANNER_PROMPT: &str = include_str!("../prompts/planner.md");
pub(crate) const WORKER_PROMPT: &str = include_str!("../prompts/worker.md");
pub(crate) const FOREMAN_REVIEW_PROMPT: &str = include_str!("../prompts/foreman_review.md");
pub(crate) const FOREMAN_REPAIR_PROMPT: &str = include_str!("../prompts/foreman_repair.md");
pub(crate) const FOREMAN_BATCH_PROMPT: &str = include_str!("../prompts/foreman_batch.md");

pub(crate) const TASK_SCHEMA_JSON: &str = include_str!("../schemas/task.schema.json");
pub(crate) const MASTER_SCHEMA: &str = include_str!("../schemas/master.schema.json");
pub(crate) const PLANNER_SCHEMA: &str = include_str!("../schemas/planner.schema.json");
pub(crate) const WORKER_SCHEMA: &str = include_str!("../schemas/worker.schema.json");
pub(crate) const FOREMAN_REVIEW_SCHEMA: &str =
    include_str!("../schemas/foreman_review.schema.json");
pub(crate) const FOREMAN_REPAIR_SCHEMA: &str =
    include_str!("../schemas/foreman_repair.schema.json");
pub(crate) const FOREMAN_BATCH_SCHEMA: &str = include_str!("../schemas/foreman_batch.schema.json");
