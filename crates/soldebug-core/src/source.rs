//! Source resolution: find and compile contract sources for trace decoding.
//!
//! Tier 1: Local Foundry project (foundry.toml in CWD or --project-dir)
//! Tier 2: Etherscan/Sourcify (future)
//! Tier 3: Hybrid (future)

use eyre::{Result, WrapErr};
use foundry_common::ContractsByArtifact;
use foundry_compilers::CompilationError;
use foundry_config::Config;
use foundry_evm_traces::debug::ContractSources;
use std::path::Path;
use tracing::info;

/// Result of source resolution: compiled artifacts and source mappings.
pub struct ResolvedSources {
    /// Known contracts by artifact ID (for trace identification).
    pub known_contracts: ContractsByArtifact,
    /// Source maps and source code (for PC-to-source mapping).
    pub contract_sources: ContractSources,
}

/// Attempt to resolve sources from a local Foundry project.
///
/// Looks for `foundry.toml` in the given directory (or CWD) and compiles the project.
pub fn resolve_local_sources(project_dir: Option<&Path>) -> Result<Option<ResolvedSources>> {
    // Load config from the project directory or CWD
    let config = if let Some(dir) = project_dir {
        Config::load_with_root(dir)?.sanitized()
    } else {
        // Try CWD - Config::load() will find foundry.toml if present
        let config = match Config::load() {
            Ok(c) => c.sanitized(),
            Err(_) => {
                info!("No foundry.toml found, skipping local sources");
                return Ok(None);
            }
        };
        // Check if we actually found a foundry.toml
        if !config.root.join("foundry.toml").exists() {
            info!("No foundry.toml found in current directory, skipping local sources");
            return Ok(None);
        }
        config
    };

    info!(root = %config.root.display(), "Compiling local Foundry project");

    let project = config.project().wrap_err("Failed to create project from config")?;
    let output = project.compile()?;

    if output.has_compiler_errors() {
        let mut errors = String::new();
        for err in output.output().errors.iter().filter(|e| e.is_error()) {
            errors.push_str(&format!("{err}\n"));
        }
        eyre::bail!("Compilation failed:\n{errors}");
    }

    let known_contracts = ContractsByArtifact::new(
        output
            .artifact_ids()
            .map(|(id, artifact)| (id, artifact.clone().into())),
    );

    let contract_sources =
        ContractSources::from_project_output(&output, project.root(), None)
            .wrap_err("Failed to build contract sources from compiler output")?;

    info!(
        contracts = known_contracts.len(),
        "Local source resolution complete"
    );

    Ok(Some(ResolvedSources {
        known_contracts,
        contract_sources,
    }))
}
