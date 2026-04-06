mod cli;

use clap::Parser;
use cli::{Cli, OutputFormat};
use eyre::Result;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    run(cli).await
}

async fn run(cli: Cli) -> Result<()> {
    let tx_hash: alloy_primitives::B256 = cli.tx_hash.parse().map_err(|_| {
        eyre::eyre!(
            "Invalid transaction hash: '{}'. Expected a 0x-prefixed 32-byte hex string.",
            cli.tx_hash
        )
    })?;

    let rpc_url = cli.resolve_rpc_url();
    let debug_mode = matches!(cli.output, OutputFormat::Interactive);

    // Step 1: Resolve local sources (if foundry.toml exists)
    eprintln!("Resolving contract sources...");
    let sources = soldebug_core::source::resolve_local_sources(cli.project_dir.as_deref())?;

    if sources.is_some() {
        eprintln!("  Local Foundry project compiled successfully.");
    } else {
        eprintln!("  No local project found, proceeding without source maps.");
    }

    // Step 2: Replay the transaction
    eprintln!("Replaying transaction {}...", cli.tx_hash);
    let replay =
        soldebug_core::replay::replay_transaction(&rpc_url, tx_hash, cli.quick, debug_mode).await?;

    let traces = match replay.traces {
        Some(t) => t,
        None => {
            eprintln!("No traces collected. The transaction may have been a simple transfer.");
            return Ok(());
        }
    };

    // Step 3: Decode traces into structured data
    eprintln!("Decoding traces...");
    let mut config = foundry_config::Config::load()
        .unwrap_or_default()
        .sanitized();

    // Set Etherscan API key if provided
    if let Some(ref key) = cli.etherscan_key {
        config.etherscan_api_key = Some(key.clone());
    }

    let session = soldebug_core::decode::decode_traces(
        tx_hash,
        replay.success,
        replay.gas_used,
        traces,
        &replay.contracts_bytecode,
        sources,
        &config,
        replay.chain,
    )
    .await?;

    // Step 4: Output
    match cli.output {
        OutputFormat::Trace => {
            let output = soldebug_output::trace_fmt::format_trace(&session, cli.verbose);
            print!("{output}");
        }
        OutputFormat::Json => {
            let output = soldebug_output::json_fmt::format_json(&session)?;
            println!("{output}");
        }
        OutputFormat::Interactive => {
            eprintln!("Interactive TUI mode is not yet implemented. Use --output trace for now.");
            let output = soldebug_output::trace_fmt::format_trace(&session, cli.verbose);
            print!("{output}");
        }
    }

    Ok(())
}
