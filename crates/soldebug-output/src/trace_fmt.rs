//! Human-readable trace output formatter.
//!
//! Produces a Tenderly-like stack trace that is both human-readable
//! and easily parseable by LLM agents.

use soldebug_core::types::{DebugSession, FrameKind, StackFrame};
use std::fmt::Write;
use yansi::Paint;

/// Format a `DebugSession` as a human-readable stack trace.
pub fn format_trace(session: &DebugSession) -> String {
    let mut out = String::new();

    // Header
    let status = if session.success {
        "SUCCESS".green().bold().to_string()
    } else {
        "REVERTED".red().bold().to_string()
    };

    let tx_short = format!(
        "0x{}...{}",
        &format!("{:x}", session.tx_hash)[..6],
        &format!("{:x}", session.tx_hash)[58..]
    );

    writeln!(
        out,
        "Transaction {} {} (gas: {})",
        tx_short.bold(),
        status,
        format_gas(session.gas_used)
    )
    .unwrap();
    writeln!(out).unwrap();

    // Call stack
    if session.call_stack.is_empty() {
        writeln!(out, "  (no call trace available)").unwrap();
    } else {
        writeln!(out, "{}", "Call Stack:".bold()).unwrap();
        for frame in &session.call_stack {
            write_frame(&mut out, frame, 1, true);
        }
    }

    // Revert reason summary
    if let Some(ref reason) = session.revert_reason {
        writeln!(out).unwrap();
        writeln!(out, "{} {}", "Revert Reason:".red().bold(), reason).unwrap();
    }

    out
}

/// Format a `DebugSession` as plain text (no ANSI colors).
pub fn format_trace_plain(session: &DebugSession) -> String {
    let mut out = String::new();

    let status = if session.success { "SUCCESS" } else { "REVERTED" };
    let tx_short = format!(
        "0x{}...{}",
        &format!("{:x}", session.tx_hash)[..6],
        &format!("{:x}", session.tx_hash)[58..]
    );

    writeln!(
        out,
        "Transaction {} {} (gas: {})",
        tx_short,
        status,
        format_gas(session.gas_used)
    )
    .unwrap();
    writeln!(out).unwrap();

    if session.call_stack.is_empty() {
        writeln!(out, "  (no call trace available)").unwrap();
    } else {
        writeln!(out, "Call Stack:").unwrap();
        for frame in &session.call_stack {
            write_frame_plain(&mut out, frame, 1, true);
        }
    }

    if let Some(ref reason) = session.revert_reason {
        writeln!(out).unwrap();
        writeln!(out, "Revert Reason: {reason}").unwrap();
    }

    out
}

fn write_frame(out: &mut String, frame: &StackFrame, indent: usize, is_root: bool) {
    let prefix = if is_root {
        "  ".repeat(indent)
    } else {
        format!("{}+-- ", "  ".repeat(indent - 1))
    };

    // Contract.function(args)
    let addr_str = format_address(frame.address);
    let contract = frame
        .contract_name
        .as_deref()
        .unwrap_or(&addr_str);

    let call_kind_prefix = match frame.kind {
        FrameKind::DelegateCall => "[delegatecall] ",
        FrameKind::StaticCall => "[staticcall] ",
        FrameKind::Create | FrameKind::Create2 => "[create] ",
        FrameKind::Call => "",
    };

    let func = frame
        .function_name
        .as_deref()
        .unwrap_or("???");

    let args = if frame.function_args.is_empty() {
        String::new()
    } else {
        frame
            .function_args
            .iter()
            .map(|(name, val)| {
                if name.is_empty() {
                    val.clone()
                } else {
                    format!("{name}={val}")
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };

    let status_marker = if frame.success {
        ""
    } else if frame.children.iter().any(|c| !c.success) {
        "" // failure is in a child, don't mark this frame
    } else {
        " <- REVERT"
    };

    // Write the call line
    writeln!(
        out,
        "{prefix}{call_kind_prefix}{}.{}({args}){status_marker}",
        contract.cyan(),
        func.yellow(),
    )
    .unwrap();

    // Source location
    if let Some(ref loc) = frame.source_location {
        writeln!(
            out,
            "{}    at {}:{}:{}",
            "  ".repeat(indent),
            loc.file.display(),
            loc.line,
            loc.column
        )
        .unwrap();
    }

    // Revert reason on the failing leaf
    if !frame.success && frame.children.iter().all(|c| c.success) {
        if let Some(ref reason) = frame.revert_reason {
            writeln!(
                out,
                "{}    {} {}",
                "  ".repeat(indent),
                "REVERT:".red().bold(),
                reason
            )
            .unwrap();
        }
    }

    // Children
    for child in &frame.children {
        write_frame(out, child, indent + 1, false);
    }
}

fn write_frame_plain(out: &mut String, frame: &StackFrame, indent: usize, is_root: bool) {
    let prefix = if is_root {
        "  ".repeat(indent)
    } else {
        format!("{}+-- ", "  ".repeat(indent - 1))
    };

    let addr_str = format_address(frame.address);
    let contract = frame
        .contract_name
        .as_deref()
        .unwrap_or(&addr_str);

    let call_kind_prefix = match frame.kind {
        FrameKind::DelegateCall => "[delegatecall] ",
        FrameKind::StaticCall => "[staticcall] ",
        FrameKind::Create | FrameKind::Create2 => "[create] ",
        FrameKind::Call => "",
    };

    let func = frame.function_name.as_deref().unwrap_or("???");
    let args = if frame.function_args.is_empty() {
        String::new()
    } else {
        frame
            .function_args
            .iter()
            .map(|(name, val)| {
                if name.is_empty() {
                    val.clone()
                } else {
                    format!("{name}={val}")
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };

    let status_marker = if frame.success {
        ""
    } else if frame.children.iter().any(|c| !c.success) {
        ""
    } else {
        " <- REVERT"
    };

    writeln!(out, "{prefix}{call_kind_prefix}{contract}.{func}({args}){status_marker}").unwrap();

    if let Some(ref loc) = frame.source_location {
        writeln!(
            out,
            "{}    at {}:{}:{}",
            "  ".repeat(indent),
            loc.file.display(),
            loc.line,
            loc.column
        )
        .unwrap();
    }

    if !frame.success && frame.children.iter().all(|c| c.success) {
        if let Some(ref reason) = frame.revert_reason {
            writeln!(out, "{}    REVERT: {reason}", "  ".repeat(indent)).unwrap();
        }
    }

    for child in &frame.children {
        write_frame_plain(out, child, indent + 1, false);
    }
}

fn format_address(addr: alloy_primitives::Address) -> String {
    let hex = format!("{addr:?}");
    if hex.len() > 12 {
        format!("{}...{}", &hex[..6], &hex[hex.len() - 4..])
    } else {
        hex
    }
}

fn format_gas(gas: u64) -> String {
    if gas >= 1_000_000 {
        format!("{:.2}M", gas as f64 / 1_000_000.0)
    } else if gas >= 1_000 {
        format!("{:.1}K", gas as f64 / 1_000.0)
    } else {
        gas.to_string()
    }
}
