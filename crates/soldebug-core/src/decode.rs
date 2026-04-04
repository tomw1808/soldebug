//! Trace decoding: identify contracts, decode function calls, and build stack frames.

use crate::source::ResolvedSources;
use crate::types::{DebugSession, FrameKind, StackFrame};
use alloy_primitives::{Address, B256, Bytes, map::HashMap};
use eyre::Result;
use foundry_evm_traces::{
    CallTraceDecoderBuilder, CallTraceNode, DebugTraceIdentifier, Traces,
    debug::ContractSources,
    identifier::TraceIdentifiers,
};
use tracing::info;

/// Decode traces into a structured `DebugSession`.
pub async fn decode_traces(
    tx_hash: B256,
    success: bool,
    gas_used: u64,
    mut traces: Traces,
    contracts_bytecode: &HashMap<Address, Bytes>,
    sources: Option<ResolvedSources>,
    _config: &foundry_config::Config,
) -> Result<DebugSession> {
    let (known_contracts, contract_sources) = match sources {
        Some(resolved) => (Some(resolved.known_contracts), resolved.contract_sources),
        None => (None, ContractSources::default()),
    };

    // Build the decoder
    let mut builder = CallTraceDecoderBuilder::new();
    let mut identifier = TraceIdentifiers::new();

    if let Some(ref contracts) = known_contracts {
        builder = builder.with_known_contracts(contracts);
        identifier = identifier.with_local_and_bytecodes(contracts, contracts_bytecode);
    }

    let mut decoder = builder.build();

    // Identify addresses in traces
    for (_, trace) in traces.iter_mut() {
        decoder.identify(trace, &mut identifier);
    }

    // Set up debug identifier for internal function tracing
    decoder.debug_identifier = Some(DebugTraceIdentifier::new(contract_sources));

    // Decode all traces
    for (_, trace) in traces.iter_mut() {
        foundry_evm_traces::decode_trace_arena(trace, &decoder).await;
    }

    // Build stack frames from decoded traces
    let call_stack = build_stack_frames(&traces);

    // Extract revert reason from the deepest failing frame
    let revert_reason = if !success {
        extract_revert_reason(&call_stack)
    } else {
        None
    };

    info!(
        frames = call_stack.len(),
        revert = revert_reason.as_deref().unwrap_or("none"),
        "Trace decoding complete"
    );

    Ok(DebugSession {
        tx_hash,
        success,
        gas_used,
        revert_reason,
        call_stack,
        traces: Some(traces),
    })
}

/// Walk the `CallTraceArena` and build a tree of `StackFrame`s.
fn build_stack_frames(traces: &Traces) -> Vec<StackFrame> {
    let mut frames = Vec::new();
    for (_, trace) in traces {
        let nodes = trace.arena.nodes();
        if let Some(root) = nodes.first() {
            frames.push(node_to_frame(root, nodes, 0));
        }
    }
    frames
}

/// Convert a `CallTraceNode` into a `StackFrame`, recursively processing children.
fn node_to_frame(
    node: &CallTraceNode,
    all_nodes: &[CallTraceNode],
    depth: usize,
) -> StackFrame {
    let trace = &node.trace;

    // Extract decoded info (label, function signature, return data)
    let (contract_name, function_name, function_args, return_value) =
        if let Some(ref decoded) = trace.decoded {
            let label = decoded.label.clone();

            let (fname, fargs) = if let Some(ref call_data) = decoded.call_data {
                // Parse function name from signature (e.g., "transfer(address,uint256)" -> "transfer")
                let name = call_data
                    .signature
                    .split('(')
                    .next()
                    .unwrap_or(&call_data.signature)
                    .to_string();
                let args: Vec<(String, String)> = call_data
                    .args
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (format!("arg{i}"), v.clone()))
                    .collect();
                (Some(name), args)
            } else {
                (None, Vec::new())
            };

            let ret = decoded.return_data.clone();

            (label, fname, fargs, ret)
        } else {
            (None, None, Vec::new(), None)
        };

    // Build children recursively using the children indices
    let children: Vec<StackFrame> = node
        .children
        .iter()
        .filter_map(|&child_idx| {
            all_nodes.get(child_idx).map(|child_node| {
                node_to_frame(child_node, all_nodes, depth + 1)
            })
        })
        .collect();

    let revert_reason = if trace.success {
        None
    } else {
        // Use decoded return data as revert reason, or raw hex
        if let Some(ref decoded) = trace.decoded {
            decoded.return_data.clone()
        } else if !trace.output.is_empty() {
            Some(format!("0x{}", alloy_primitives::hex::encode(&trace.output)))
        } else {
            None
        }
    };

    StackFrame {
        address: trace.address,
        contract_name,
        function_name,
        function_args,
        return_value: if trace.success { return_value } else { None },
        source_location: None, // TODO: populate from PcSourceMapper
        depth,
        kind: FrameKind::from(trace.kind),
        success: trace.success,
        revert_reason,
        gas_used: trace.gas_used,
        children,
    }
}

/// Extract the revert reason from the deepest failing frame.
fn extract_revert_reason(frames: &[StackFrame]) -> Option<String> {
    for frame in frames {
        if !frame.success {
            // Try children first (deepest cause)
            if let Some(reason) = extract_revert_reason(&frame.children) {
                return Some(reason);
            }
            // Fall back to this frame's revert reason
            if let Some(ref reason) = frame.revert_reason {
                return Some(reason.clone());
            }
        }
    }
    None
}
