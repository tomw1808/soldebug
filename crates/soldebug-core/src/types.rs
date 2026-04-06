use alloy_primitives::{Address, B256};
use foundry_evm_traces::{CallKind, Traces};
use serde::Serialize;
use std::path::PathBuf;

/// Complete debug session for a transaction.
#[derive(Debug, Serialize)]
pub struct DebugSession {
    /// Transaction hash.
    pub tx_hash: B256,
    /// Whether the transaction succeeded.
    pub success: bool,
    /// Gas used by the transaction.
    pub gas_used: u64,
    /// Revert reason (if any).
    pub revert_reason: Option<String>,
    /// Structured call stack (tree of frames).
    pub call_stack: Vec<StackFrame>,
    /// Raw traces for interactive debugging.
    #[serde(skip)]
    pub traces: Option<Traces>,
}

/// A single frame in the call stack.
#[derive(Debug, Clone, Serialize)]
pub struct StackFrame {
    /// Contract address being called.
    pub address: Address,
    /// Identified contract name (e.g., "ERC20").
    pub contract_name: Option<String>,
    /// Function name (e.g., "transferFrom").
    pub function_name: Option<String>,
    /// Raw 4-byte function selector (e.g., "0x70a08231").
    /// Present when calldata >= 4 bytes, regardless of whether decoding succeeded.
    pub selector: Option<String>,
    /// Decoded function arguments as (name, value) pairs.
    pub function_args: Vec<(String, String)>,
    /// Return value (decoded).
    pub return_value: Option<String>,
    /// Source code location.
    pub source_location: Option<SourceLoc>,
    /// Call depth (0 = top-level).
    pub depth: usize,
    /// Type of call.
    pub kind: FrameKind,
    /// Whether this call succeeded.
    pub success: bool,
    /// Revert reason if this call reverted.
    pub revert_reason: Option<String>,
    /// Gas used by this call.
    pub gas_used: u64,
    /// Child calls.
    pub children: Vec<StackFrame>,
}

/// Source code location.
#[derive(Debug, Clone, Serialize)]
pub struct SourceLoc {
    /// File path (relative to project root).
    pub file: PathBuf,
    /// Line number (1-indexed).
    pub line: usize,
    /// Column number (1-indexed).
    pub column: usize,
    /// A few lines of source context around the location.
    pub source_snippet: Option<String>,
}

/// Kind of call frame.
#[derive(Debug, Clone, Copy, Serialize)]
pub enum FrameKind {
    Call,
    DelegateCall,
    StaticCall,
    Create,
    Create2,
}

impl From<CallKind> for FrameKind {
    fn from(kind: CallKind) -> Self {
        match kind {
            CallKind::Call => Self::Call,
            CallKind::DelegateCall => Self::DelegateCall,
            CallKind::StaticCall => Self::StaticCall,
            CallKind::Create => Self::Create,
            CallKind::Create2 => Self::Create2,
            _ => Self::Call,
        }
    }
}
