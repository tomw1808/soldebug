//! Source resolution: find and compile contract sources for trace decoding.
//!
//! Tier 1: Local Foundry project (foundry.toml in CWD or --project-dir)
//! Tier 2: Etherscan/Sourcify (future)
//! Tier 3: Hybrid (future)

use eyre::{Result, WrapErr};
use foundry_common::ContractsByArtifact;
use foundry_compilers::ArtifactId;
use foundry_compilers::artifacts::ConfigurableContractArtifact;
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

    let project = config
        .project()
        .wrap_err("Failed to create project from config")?;

    // Compile the project (or use cached output)
    let output = project.compile()?;

    // Check how many artifacts we got from compilation
    let compile_count = output.artifact_ids().count();

    let (known_contracts, contract_sources) = if compile_count > 0 {
        // Compilation returned artifacts (fresh build or partial recompile)
        let known = ContractsByArtifact::new(
            output
                .artifact_ids()
                .map(|(id, artifact)| (id, artifact.clone().into())),
        );
        let sources = ContractSources::from_project_output(&output, project.root(), None)
            .wrap_err("Failed to build contract sources from compiler output")?;
        (known, sources)
    } else {
        // Compilation was fully cached - read artifacts directly from out/ directory
        eprintln!("  No new compilation needed, reading cached artifacts...");
        read_cached_artifacts(&config, &project)?
    };

    info!(
        contracts = known_contracts.len(),
        "Local source resolution complete"
    );

    Ok(Some(ResolvedSources {
        known_contracts,
        contract_sources,
    }))
}

/// Read cached compilation artifacts from the project's output directory.
///
/// When `project.compile()` returns nothing because everything is cached,
/// we walk the `out/` directory and parse the artifact JSON files directly.
fn read_cached_artifacts(
    _config: &Config,
    project: &foundry_compilers::Project,
) -> Result<(ContractsByArtifact, ContractSources)> {
    let artifacts_dir = &project.paths.artifacts;

    if !artifacts_dir.exists() {
        eyre::bail!(
            "Artifacts directory {} does not exist. Run `forge build` first.",
            artifacts_dir.display()
        );
    }

    let mut artifacts: Vec<(ArtifactId, ConfigurableContractArtifact)> = Vec::new();

    // Walk the out/ directory: structure is out/<FileName.sol>/<ContractName>.json
    for entry in walkdir::WalkDir::new(artifacts_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
    {
        let path = entry.path();

        // Parse the artifact
        let content = std::fs::read_to_string(path)
            .wrap_err_with(|| format!("Failed to read artifact: {}", path.display()))?;
        let artifact: ConfigurableContractArtifact = match serde_json::from_str(&content) {
            Ok(a) => a,
            Err(_) => continue, // Skip non-artifact JSON files (e.g., build-info)
        };

        // Build an ArtifactId from the file path
        // Path pattern: out/<SourceFile.sol>/<ContractName>.json
        let contract_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string();

        let source_file = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown.sol")
            .to_string();

        // Try to find the actual source path relative to project root
        let source_path = find_source_file(project.root(), &source_file)
            .unwrap_or_else(|| project.root().join("src").join(&source_file));

        let id = ArtifactId {
            path: source_path,
            name: contract_name,
            source: path.to_path_buf(),
            version: semver::Version::new(0, 8, 0), // Placeholder; exact version not critical for matching
            build_id: String::new(),
            profile: String::from("default"),
        };

        artifacts.push((id, artifact));
    }

    eprintln!(
        "  Read {} cached artifacts from {}",
        artifacts.len(),
        artifacts_dir.display()
    );

    let known_contracts = ContractsByArtifact::new(
        artifacts
            .iter()
            .map(|(id, artifact)| (id.clone(), artifact.clone().into())),
    );

    // For contract sources, we need to do a compile to get proper source maps.
    // For now, return empty sources - the ABI matching will still work.
    let contract_sources = ContractSources::default();

    Ok((known_contracts, contract_sources))
}

/// Find the actual source file path by searching the project's source directories.
fn find_source_file(root: &Path, filename: &str) -> Option<std::path::PathBuf> {
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        if entry.file_name().to_str() == Some(filename) {
            return Some(entry.path().to_path_buf());
        }
    }
    None
}
