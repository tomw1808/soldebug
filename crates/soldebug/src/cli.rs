//! CLI argument definitions.

use clap::Parser;

/// Output format for the CLI.
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum OutputFormat {
    /// Human-readable stack trace (default, LLM-friendly).
    #[default]
    Trace,
    /// Machine-readable JSON.
    Json,
    /// Interactive TUI debugger.
    Interactive,
}

/// Modern Solidity transaction debugger.
///
/// Replays a transaction against a forked chain and produces a detailed stack trace
/// with source-mapped contract locations. Designed for both human and LLM consumption.
#[derive(Parser, Debug)]
#[command(name = "soldebug", version, about)]
pub struct Cli {
    /// The transaction hash to debug.
    pub tx_hash: String,

    /// RPC endpoint URL.
    ///
    /// Defaults to ETH_RPC_URL env var or http://localhost:8545.
    #[arg(long, short = 'r', env = "ETH_RPC_URL")]
    pub rpc_url: Option<String>,

    /// Foundry project directory (defaults to CWD if foundry.toml is present).
    #[arg(long, short = 'p')]
    pub project_dir: Option<std::path::PathBuf>,

    /// Output format.
    #[arg(long, short = 'o', default_value = "trace")]
    pub output: OutputFormat,

    /// Skip replaying preceding transactions in the block.
    ///
    /// Faster but may produce different results than the live execution.
    #[arg(long, short = 'q')]
    pub quick: bool,

    /// Etherscan API key for fetching verified contract sources.
    #[arg(long, env = "ETHERSCAN_API_KEY")]
    pub etherscan_key: Option<String>,

    /// Chain identifier (e.g., "mainnet", "polygon", "arbitrum").
    #[arg(long)]
    pub chain: Option<String>,

    /// Increase output verbosity (-v: addresses+gas, -vv: selectors+full addresses).
    #[arg(long, short = 'v', action = clap::ArgAction::Count)]
    pub verbose: u8,
}

impl Cli {
    /// Resolve the RPC URL from CLI args or defaults.
    pub fn resolve_rpc_url(&self) -> String {
        if let Some(ref url) = self.rpc_url {
            return url.clone();
        }
        "http://localhost:8545".to_string()
    }
}
