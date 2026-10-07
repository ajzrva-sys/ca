//! Planner role runner.
//!
//! Target home for roadmap/queue replenishment prompts and parsed work orders.
//! Planner should manufacture `TaskSpec` objects only when Rust cannot safely
//! promote existing backlog or continue an active retry lane.
