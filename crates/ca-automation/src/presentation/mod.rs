//! Terminal rendering and prompt-size utilities.
//!
//! Pure presentation code extracted from `app.rs`.  These functions format,
//! truncate, and print; they do not mutate authoritative state.

mod formatting;
mod live;
mod prompt;
mod status;
mod theme;

// Re-export public API so callers can use `crate::presentation::foo`.
pub(crate) use formatting::{
    display_path, format_count, format_duration, role_label, strip_ansi, truncate_center,
    truncate_inline, wrap_terminal_text,
};
pub(crate) use live::{
    friendly_command, is_codex_tool_stderr_detail, is_high_signal_success_output,
    is_live_start_command, is_non_actionable_stderr, is_rg_no_matches, is_swiftpm_command,
    is_swiftpm_sandbox_failure, preview_text, render_jsonl_stream, render_stderr_stream,
    rg_no_matches_activity, summarize_codex_tool_stderr,
};
pub(crate) use prompt::{
    build_planner_queue_brief, render_project_manager_compact_prompt, render_prompt,
    render_prompt_limited, render_schema, PromptLimits,
};
pub(crate) use status::{
    mission_summary_rows, print_doctor_receipt, print_status, report_agent_role_warnings,
    report_role_tier_warnings, role_strength_score,
};
pub(crate) use theme::{
    frame_rule, frame_width, print_codex_role_header, print_frame_row, print_role_header, Theme,
};
