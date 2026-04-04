//! Transaction replay engine.
//!
//! Fetches a transaction from the chain, forks the state at the parent block,
//! replays preceding transactions, and executes the target transaction with
//! tracing enabled.

use alloy_consensus::{BlockHeader, Transaction};
use alloy_evm::FromRecoveredTx;
use alloy_network::{AnyNetwork, TransactionResponse};
use alloy_primitives::{Address, Bytes, U256, map::HashMap};
use alloy_provider::Provider;
use alloy_rpc_types::BlockTransactions;
use eyre::{Result, WrapErr};
use foundry_compilers::artifacts::EvmVersion;
use foundry_config::Config;
use foundry_evm::{
    executors::{EvmError, TracingExecutor},
    opts::EvmOpts,
    traces::{InternalTraceMode, TraceKind, TraceMode, Traces},
};
use foundry_evm_core::evm::EthEvmNetwork;
use revm::{DatabaseRef, context::TxEnv};
use tracing::{info, trace};

/// Result of replaying a transaction.
pub struct ReplayResult {
    /// Whether the transaction succeeded.
    pub success: bool,
    /// Collected traces.
    pub traces: Option<Traces>,
    /// Gas used.
    pub gas_used: u64,
    /// Bytecodes at addresses involved in the trace.
    pub contracts_bytecode: HashMap<Address, Bytes>,
}

/// Replay a transaction and collect traces.
///
/// This forks the chain at the parent block, replays preceding transactions,
/// and executes the target transaction with full call tracing.
pub async fn replay_transaction(
    rpc_url: &str,
    tx_hash: alloy_primitives::B256,
    quick: bool,
    debug_mode: bool,
) -> Result<ReplayResult> {
    // Build provider
    let provider = alloy_provider::ProviderBuilder::new()
        .network::<AnyNetwork>()
        .connect(rpc_url)
        .await
        .wrap_err("Failed to connect to RPC")?;

    // Fetch the transaction
    let tx = provider
        .get_transaction_by_hash(tx_hash)
        .await
        .wrap_err("Failed to fetch transaction")?
        .ok_or_else(|| eyre::eyre!("Transaction not found: {tx_hash:?}"))?;

    let tx_block_number = tx
        .block_number
        .ok_or_else(|| eyre::eyre!("Transaction may still be pending: {tx_hash:?}"))?;

    info!(block = tx_block_number, "Fetched transaction");

    // Configure fork at parent block
    let mut config = Config::default();
    config.eth_rpc_url = Some(rpc_url.to_string());
    config.fork_block_number = Some(tx_block_number - 1);

    let mut evm_opts = EvmOpts::default();
    evm_opts.fork_url = Some(rpc_url.to_string());
    evm_opts.fork_block_number = Some(tx_block_number - 1);
    // Set a generous memory limit (2^32 bytes = 4GB) to avoid MemoryLimitOOG
    evm_opts.memory_limit = 1 << 32;

    // Get fork material (env, tx_env, fork, chain, networks)
    let (mut evm_env, tx_env, fork, _chain, networks) =
        TracingExecutor::<EthEvmNetwork>::get_fork_material(&mut config, evm_opts.clone()).await?;

    // Detect EVM version from block
    let block = provider
        .get_block(tx_block_number.into())
        .full()
        .await
        .ok()
        .flatten();

    let evm_version = block.as_ref().and_then(|b| {
        if b.header.excess_blob_gas().is_some() {
            Some(EvmVersion::Prague)
        } else {
            None
        }
    });

    // The block env is already set by get_fork_material based on the fork block.
    // We just update the block number to be the actual tx block (not parent).
    evm_env.block_env.number = U256::from(tx_block_number);

    evm_env.cfg_env.disable_block_gas_limit = true;
    evm_env.cfg_env.limit_contract_code_size = None;

    // Set up trace mode
    let trace_mode = TraceMode::Call
        .with_debug(debug_mode)
        .with_decode_internal(InternalTraceMode::Full);

    // Create the tracing executor
    let mut executor = TracingExecutor::<EthEvmNetwork>::new(
        (evm_env.clone(), tx_env),
        fork,
        evm_version,
        trace_mode,
        networks,
        evm_opts.create2_deployer,
        None,
    )?;

    evm_env.cfg_env.set_spec(executor.spec_id());

    // Replay preceding transactions in the block
    if !quick {
        if let Some(ref block) = block {
            let BlockTransactions::Full(ref txs) = block.transactions else {
                eyre::bail!("Block transactions not available in full format");
            };

            // Find position of our tx in the block
            let tx_position = txs
                .iter()
                .position(|t| t.tx_hash() == tx_hash)
                .unwrap_or(txs.len());

            if tx_position > 0 {
                eprintln!(
                    "  Replaying {tx_position} preceding transactions in block {} (fetching state from RPC)...",
                    evm_env.block_env.number
                );
                if tx_position > 50 {
                    eprintln!(
                        "  Hint: use --quick to skip this step (faster, but may produce slightly different results)"
                    );
                }
            }

            for (idx, tx_in_block) in txs.iter().enumerate() {
                if tx_in_block.tx_hash() == tx_hash {
                    break;
                }

                if tx_position > 10 && (idx + 1) % 10 == 0 {
                    eprintln!("  [{}/{}] replaying...", idx + 1, tx_position);
                }

                let tx_env = tx_in_block
                    .as_envelope()
                    .map_or(Default::default(), |envelope| {
                        TxEnv::from_recovered_tx(envelope, tx_in_block.from())
                    });

                let mut env = evm_env.clone();
                env.cfg_env.disable_balance_check = true;

                if Transaction::to(tx_in_block).is_some() {
                    trace!(tx = ?tx_in_block.tx_hash(), "Replaying call transaction");
                    let _ = executor.transact_with_env(env, tx_env);
                } else {
                    trace!(tx = ?tx_in_block.tx_hash(), "Replaying create transaction");
                    match executor.deploy_with_env(env, tx_env, None) {
                        Err(EvmError::Execution(_)) => (), // Reverted txs are fine
                        Err(e) => return Err(e.into()),
                        Ok(_) => (),
                    }
                }
            }
        }
    }

    // Execute the target transaction
    let result_tx_env = tx
        .as_envelope()
        .map_or(Default::default(), |envelope| {
            TxEnv::from_recovered_tx(envelope, tx.from())
        });

    let raw_result = if Transaction::to(&tx).is_some() {
        info!("Executing target call transaction");
        executor.transact_with_env(evm_env, result_tx_env)?
    } else {
        info!("Executing target create transaction");
        executor.deploy_with_env(evm_env, result_tx_env, None)?.raw
    };

    // Gather bytecodes from all addresses in traces
    let mut contracts_bytecode = HashMap::default();
    if let Some(ref traces) = raw_result.traces {
        for node in traces.arena.nodes() {
            for addr in [node.trace.address, node.trace.caller] {
                if !addr.is_zero() && !contracts_bytecode.contains_key(&addr) {
                    if let Ok(Some(info)) = executor.backend().basic_ref(addr) {
                        if let Some(code) = info.code {
                            let bytes = code.bytes();
                            if !bytes.is_empty() {
                                contracts_bytecode.insert(addr, bytes);
                            }
                        }
                    }
                }
            }
        }
    }

    let traces = raw_result
        .traces
        .map(|arena| vec![(TraceKind::Execution, arena)]);

    Ok(ReplayResult {
        success: !raw_result.reverted,
        traces,
        gas_used: raw_result.gas_used,
        contracts_bytecode,
    })
}
