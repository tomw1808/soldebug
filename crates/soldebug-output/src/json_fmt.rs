//! JSON output formatter for LLM/machine consumption.

use soldebug_core::types::DebugSession;

/// Format a `DebugSession` as pretty-printed JSON.
pub fn format_json(session: &DebugSession) -> eyre::Result<String> {
    Ok(serde_json::to_string_pretty(session)?)
}

/// Format a `DebugSession` as compact JSON (one line).
pub fn format_json_compact(session: &DebugSession) -> eyre::Result<String> {
    Ok(serde_json::to_string(session)?)
}
