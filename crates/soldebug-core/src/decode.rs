//! Trace decoding: identify contracts, decode function calls, and build stack frames.

use crate::source::ResolvedSources;
use crate::types::{DebugSession, FrameKind, StackFrame};
use alloy_primitives::{Address, B256, Bytes, map::HashMap};
use eyre::Result;
use foundry_common::ContractsByArtifact;
use foundry_evm_traces::{
    CallTraceDecoderBuilder, CallTraceNode, DebugTraceIdentifier, Traces, debug::ContractSources,
    identifier::TraceIdentifiers,
};
use tracing::info;

/// Decode traces into a structured `DebugSession`.
#[allow(clippy::too_many_arguments)]
pub async fn decode_traces(
    tx_hash: B256,
    success: bool,
    gas_used: u64,
    mut traces: Traces,
    contracts_bytecode: &HashMap<Address, Bytes>,
    sources: Option<ResolvedSources>,
    config: &foundry_config::Config,
    chain: foundry_config::Chain,
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

    // Phase 0: Add Etherscan/Sourcify external identifier if configured
    identifier = identifier.with_external(config, Some(chain))?;

    let mut decoder = builder.build();

    // Phase 1: Bytecode-based identification + external (Etherscan/Sourcify)
    for (_, trace) in traces.iter_mut() {
        decoder.identify(trace, &mut identifier);
    }

    // Phase 2: ABI selector-based fallback for unidentified addresses
    // This handles proxies and contracts compiled with different settings
    if let Some(ref contracts) = known_contracts {
        let unidentified_count = identify_by_abi_selectors(&mut decoder, &traces, contracts);
        if unidentified_count > 0 {
            eprintln!(
                "  ABI selector matching identified {unidentified_count} additional contracts"
            );
        }
    }

    // Log final identification results
    let identified_count = decoder.contracts.len();
    let total_addresses: std::collections::HashSet<_> = traces
        .iter()
        .flat_map(|(_, t)| t.arena.nodes().iter().map(|n| n.trace.address))
        .filter(|a| !a.is_zero())
        .collect();
    let unidentified: Vec<_> = total_addresses
        .iter()
        .filter(|a| !decoder.contracts.contains_key(*a))
        .collect();
    if !unidentified.is_empty() {
        eprintln!(
            "  Identified {identified_count}/{} unique addresses ({} unidentified)",
            total_addresses.len(),
            unidentified.len()
        );
    } else {
        eprintln!("  All {identified_count} addresses identified.");
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

/// Fallback identification: match unidentified addresses by comparing the function
/// selectors in their calldata against known contract ABIs.
///
/// This handles UUPS/transparent proxies where the bytecode at the proxy address
/// doesn't match any local artifact, but the delegate call target's function
/// signatures do match a known contract.
fn identify_by_abi_selectors(
    decoder: &mut foundry_evm_traces::CallTraceDecoder,
    traces: &Traces,
    known_contracts: &ContractsByArtifact,
) -> usize {
    use alloy_json_abi::JsonAbi;
    use alloy_primitives::FixedBytes;

    // Build a map: 4-byte selector -> Vec<(contract_name, abi)>
    let mut selector_to_contracts: HashMap<FixedBytes<4>, Vec<(String, &JsonAbi)>> =
        HashMap::default();
    for (id, contract) in known_contracts.iter() {
        for func in contract.abi.functions() {
            selector_to_contracts
                .entry(func.selector())
                .or_default()
                .push((id.name.clone(), &contract.abi));
        }
    }

    let mut newly_identified = 0;

    for (_, trace) in traces.iter() {
        for node in trace.arena.nodes() {
            let addr = node.trace.address;

            // Skip already-identified and zero addresses
            if addr.is_zero() || decoder.contracts.contains_key(&addr) {
                continue;
            }

            // Skip if calldata is too short for a selector
            if node.trace.data.len() < 4 {
                continue;
            }

            // Extract 4-byte selector from calldata
            let selector: FixedBytes<4> = node.trace.data[..4].try_into().unwrap();

            // Find matching contracts
            if let Some(matches) = selector_to_contracts.get(&selector) {
                // Pick the best match: prefer contracts with the most matching selectors
                // across all calls to this address in the trace
                let best = find_best_abi_match(addr, traces, &selector_to_contracts, matches);

                if let Some((name, abi)) = best {
                    // Register with the decoder
                    decoder.contracts.insert(addr, name.to_string());
                    decoder.labels.insert(addr, name.clone());

                    // Register all functions and events from this ABI
                    for func in abi.functions() {
                        decoder.push_function(func.clone());
                    }
                    for event in abi.events() {
                        decoder.push_event(event.clone());
                    }
                    for error in abi.errors() {
                        decoder.revert_decoder.push_error(error.clone());
                    }

                    newly_identified += 1;
                }
            }
        }
    }

    newly_identified
}

/// Given an address and candidate contract matches, find the best match by counting
/// how many distinct selectors used at this address exist in each candidate ABI.
fn find_best_abi_match<'a>(
    addr: Address,
    traces: &Traces,
    _selector_to_contracts: &HashMap<
        alloy_primitives::FixedBytes<4>,
        Vec<(String, &'a alloy_json_abi::JsonAbi)>,
    >,
    candidates: &[(String, &'a alloy_json_abi::JsonAbi)],
) -> Option<(String, &'a alloy_json_abi::JsonAbi)> {
    if candidates.len() == 1 {
        let (name, abi) = &candidates[0];
        return Some((name.clone(), *abi));
    }

    // Collect all selectors used at this address
    let mut selectors_at_addr: std::collections::HashSet<alloy_primitives::FixedBytes<4>> =
        std::collections::HashSet::new();
    for (_, trace) in traces {
        for node in trace.arena.nodes() {
            if node.trace.address == addr && node.trace.data.len() >= 4 {
                let sel: alloy_primitives::FixedBytes<4> = node.trace.data[..4].try_into().unwrap();
                selectors_at_addr.insert(sel);
            }
        }
    }

    // Score each candidate by how many selectors it covers
    let mut best_score = 0usize;
    let mut best: Option<(String, &alloy_json_abi::JsonAbi)> = None;

    for (name, abi) in candidates {
        let abi_selectors: std::collections::HashSet<_> =
            abi.functions().map(|f| f.selector()).collect();
        let score = selectors_at_addr
            .iter()
            .filter(|s| abi_selectors.contains(*s))
            .count();
        if score > best_score {
            best_score = score;
            best = Some((name.clone(), *abi));
        }
    }

    best
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
fn node_to_frame(node: &CallTraceNode, all_nodes: &[CallTraceNode], depth: usize) -> StackFrame {
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
            all_nodes
                .get(child_idx)
                .map(|child_node| node_to_frame(child_node, all_nodes, depth + 1))
        })
        .collect();

    let revert_reason = if trace.success {
        None
    } else {
        // Use decoded return data as revert reason, or raw hex
        if let Some(ref decoded) = trace.decoded {
            decoded.return_data.clone()
        } else if !trace.output.is_empty() {
            Some(format!(
                "0x{}",
                alloy_primitives::hex::encode(&trace.output)
            ))
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
