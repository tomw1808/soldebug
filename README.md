# soldebug

A modern Solidity transaction debugger for the Foundry ecosystem. Takes a transaction hash, replays it against a forked chain, and produces a detailed stack trace with decoded function calls, arguments, and revert reasons.

Designed for both human developers and LLM agents debugging smart contracts.

## What it does

**Simple reverting transaction:**

```
$ soldebug 0xe1c962... --rpc-url https://sepolia.infura.io/v3/... --project-dir ./myproject

Transaction 0xe1c962...b53fb6 REVERTED (gas: 29.8K)

Call Stack:
  TestToken.mint(arg0=0xdEadDEAD..., arg1=900000000000000000000000) <- REVERT
      REVERT: MaxSupplyExceeded(900000000000000000000000, 500000000000000000000000)

Revert Reason: MaxSupplyExceeded(900000000000000000000000, 500000000000000000000000)
```

**Complex UUPS proxy transaction with external contracts:**

```
$ soldebug 0x442eaa... --rpc-url https://sepolia.infura.io/v3/... \
    --project-dir ./universal-private-dollar --etherscan-key YOUR_KEY --quick

Transaction 0x442eaa...78a20a REVERTED (gas: 421.0K)

Call Stack:
  ERC1967Proxy.fallback(...)
  +-- [delegatecall] IStabilizerNFT.mintUPD(addr, PriceAttestationQuery{...})
    +-- [staticcall] ERC20.balanceOf(ERC1967Proxy: [0xFa98...])
      +-- [delegatecall] Lido.balanceOf(ERC1967Proxy: [0xFa98...])
    +-- ERC20.submit(0x000...)
      +-- [delegatecall] Lido.submit(0x000...)
    +-- ERC1967Proxy.fallback(PriceAttestationQuery{...})
      +-- [delegatecall] PriceOracle.attestationService(...)
        +-- [staticcall] PRECOMPILES.ecrecover(...)
    +-- [delegatecall] 0xbce1...4262.???() <- REVERT
          REVERT: call to non-contract address 0x000...000
      +-- IStabilizerEscrow.unallocatedStETH()
        +-- [delegatecall] StabilizerEscrow.unallocatedStETH()
      +-- [delegatecall] CollateralMathLib.stabilizerStEthNeeded(...)
      +-- [delegatecall] CollateralMathLib.ethToUPD(...)
      +-- StabilizerEscrow.withdrawForAllocation(...)
        +-- [delegatecall] Lido.transfer(...)
      +-- PositionEscrow.addCollateralFromStabilizer(...)
      +-- UPDToken.mint(...)

Revert Reason: call to non-contract address 0x000...000
```

soldebug replays the exact transaction execution using [revm](https://github.com/bluealloy/revm), decodes every call using contract ABIs, identifies contracts by name (including through proxies), and renders a Tenderly-style call tree.

## Features

- **Transaction replay** - forks chain state at the parent block, replays preceding transactions, then executes the target tx with full tracing (with progress reporting)
- **Three-tier contract identification:**
  - **Bytecode matching** - matches deployed bytecode against local compilation artifacts
  - **ABI selector matching** - identifies proxy implementation contracts by matching function selectors against known ABIs (handles UUPS, transparent proxies, etc.)
  - **Etherscan/Sourcify fetching** - fetches verified sources for external contracts (e.g., Lido, OpenZeppelin) when an API key is provided
- **ABI decoding** - decodes function calls, arguments, return values, events, and custom errors
- **Custom error decoding** - recognizes Solidity custom errors like `MaxSupplyExceeded(uint256, uint256)` and OpenZeppelin errors like `OwnableUnauthorizedAccount(address)`
- **Proxy support** - resolves UUPS and transparent proxy patterns, showing both the proxy entry point and the implementation contract
- **Multiple output formats** - human-readable trace (default), JSON for machine/LLM consumption
- **Local source resolution** - automatically detects and compiles Foundry projects, including reading cached artifacts from `out/`
- **Works against any EVM chain** - tested on local Anvil and Sepolia testnet
- **Quick mode** - skip preceding transaction replay for faster results (`--quick`)

## Installation

### Quick install (recommended)

```bash
curl -L https://raw.githubusercontent.com/USER/soldebug/main/soldebugup/install | bash
```

This downloads the latest prebuilt binary for your platform and installs it to `~/.soldebug/bin/`. Supports macOS (Apple Silicon & Intel) and Linux (x86_64 & ARM64).

To install a specific version:

```bash
SOLDEBUG_VERSION=v0.1.0 curl -L https://raw.githubusercontent.com/USER/soldebug/main/soldebugup/install | bash
```

> **Note:** Replace `USER` with the actual GitHub username/org once the repo is published.

### Download binary manually

Grab a prebuilt binary from the [Releases](https://github.com/USER/soldebug/releases) page:

| Platform | Archive |
|----------|---------|
| macOS (Apple Silicon) | `soldebug-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
| macOS (Intel) | `soldebug-vX.Y.Z-x86_64-apple-darwin.tar.gz` |
| Linux (x86_64) | `soldebug-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| Linux (ARM64) | `soldebug-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz` |

```bash
# Example: macOS Apple Silicon
tar xzf soldebug-v0.1.0-aarch64-apple-darwin.tar.gz
sudo mv soldebug /usr/local/bin/
```

### Build from source

Requires Rust nightly (1.93+) and [Foundry](https://getfoundry.sh) installed for `solc` management.

```bash
git clone --recursive <repo-url>
cd soldebug
cargo build --release
```

The `--recursive` flag is important - it fetches the Foundry submodule in `lib/foundry/`.

The binary will be at `target/release/soldebug`.

## Usage

```
soldebug <TX_HASH> [OPTIONS]
```

### Options

| Flag | Description |
|------|-------------|
| `-r, --rpc-url <URL>` | RPC endpoint (default: `ETH_RPC_URL` env var, or `http://localhost:8545`) |
| `-p, --project-dir <PATH>` | Foundry project directory for local source resolution |
| `-o, --output <FORMAT>` | Output format: `trace` (default), `json`, `interactive` |
| `-q, --quick` | Skip replaying preceding txs in the block (faster, less accurate) |
| `--etherscan-key <KEY>` | Etherscan API key for fetching external contract sources (env: `ETHERSCAN_API_KEY`) |
| `--chain <CHAIN>` | Chain identifier (auto-detected from RPC) |
| `-v, --verbose` | Verbose output |

### Examples

**Debug a reverting transaction with local sources:**

```bash
soldebug 0xabc123... --rpc-url http://localhost:8545 --project-dir ./my-foundry-project
```

**Debug against a testnet with Etherscan for external contract resolution:**

```bash
soldebug 0xabc123... \
  --rpc-url https://sepolia.infura.io/v3/YOUR_KEY \
  --project-dir ./my-project \
  --etherscan-key YOUR_ETHERSCAN_KEY
```

**Quick mode (skip preceding tx replay, much faster for busy blocks):**

```bash
soldebug 0xabc123... --rpc-url http://localhost:8545 --project-dir ./my-project -q
```

**JSON output for LLM agent consumption:**

```bash
soldebug 0xabc123... --rpc-url http://localhost:8545 --project-dir ./my-project --output json
```

### Output formats

**`--output trace`** (default) - human-readable call tree, optimized for terminal and LLM readability:

```
Transaction 0x8e323a...e90132 SUCCESS (gas: 51.9K)

Call Stack:
  TestToken.transfer(arg0=0x000...dEaD, arg1=1000000000000000000000)
```

**`--output json`** - structured JSON with all decoded data:

```json
{
  "tx_hash": "0xe1c962...",
  "success": false,
  "gas_used": 29834,
  "revert_reason": "MaxSupplyExceeded(900000000000000000000000, 500000000000000000000000)",
  "call_stack": [
    {
      "address": "0xda8f3b...",
      "contract_name": "TestToken",
      "function_name": "mint",
      "kind": "Call",
      "success": false,
      "children": []
    }
  ]
}
```

### How --quick works

By default, soldebug replays all transactions in the block preceding your target transaction to reconstruct the exact state. On remote RPCs, each state access is an HTTP round-trip, so a block with 176 transactions can take minutes.

With `--quick`, it forks state at the parent block and executes only your transaction. This is accurate for most cases - it only matters when a preceding transaction in the same block modifies state your transaction depends on.

## Architecture

```
soldebug/
  lib/foundry/                 # Foundry as git submodule
  crates/
    soldebug/                  # CLI binary (clap argument parsing)
      src/main.rs              # Entry point, orchestrates the pipeline
      src/cli.rs               # CLI argument definitions
    soldebug-core/             # Core library
      src/replay.rs            # Transaction replay engine (TracingExecutor + revm)
      src/source.rs            # Source resolution (compile + cached artifact reading)
      src/decode.rs            # Trace decoding (three-tier identification + StackFrame building)
      src/types.rs             # Data types (DebugSession, StackFrame, SourceLoc)
    soldebug-output/           # Output formatting
      src/trace_fmt.rs         # Human-readable Tenderly-style call tree
      src/json_fmt.rs          # JSON serialization
```

### How it works

1. **Source resolution** - if `--project-dir` points to a Foundry project (or CWD has `foundry.toml`), compile it and extract ABIs + source maps. Reads cached artifacts from `out/` when the compiler reports nothing to recompile.
2. **Transaction replay** - fetch the transaction from the RPC, fork chain state at the parent block, optionally replay all preceding transactions in the block, then execute the target transaction with `TracingInspector` enabled.
3. **Three-tier contract identification:**
   - **Tier 1: Bytecode matching** - compare deployed bytecodes against local compilation artifacts (exact and near-exact matching with diff scoring)
   - **Tier 2: ABI selector matching** - for unidentified addresses, match the 4-byte function selectors in calldata against all known contract ABIs. This catches proxy implementations where bytecodes differ due to immutables/constructor args.
   - **Tier 3: Etherscan/Sourcify** - fetch verified source metadata from Etherscan and Sourcify for external contracts not in the local project (e.g., Lido, Uniswap). Rate-limited with automatic backoff.
4. **Trace decoding** - decode function calls, arguments, return values, events, and custom errors using the identified ABIs.
5. **Output** - render the decoded call tree in the requested format.

### Dependency strategy

soldebug depends on Foundry's internal crates as path dependencies via a git submodule (`lib/foundry/`). This gives us access to battle-tested infrastructure:

- **`foundry-evm`** - `TracingExecutor`, `Executor`, inspector stack
- **`foundry-evm-traces`** - `CallTraceDecoder`, `CallTraceArena`, `ContractSources`, `ExternalIdentifier`, trace identification
- **`foundry-evm-core`** - `Backend`, fork DB, EVM configuration
- **`foundry-config`** - `Config` for reading `foundry.toml`
- **`foundry-compilers`** - Solidity compiler integration, artifact parsing

The workspace's `[patch.crates-io]` section replicates Foundry's dependency pins (alloy, revm, solar, etc.) to ensure version compatibility.

## Roadmap

### Phase 1: Stack trace output

- [x] CLI with tx hash input and RPC URL
- [x] Transaction replay via `TracingExecutor`
- [x] Local Foundry project source resolution (including cached artifacts)
- [x] Contract identification (bytecode matching)
- [x] Human-readable trace output
- [x] JSON output for LLM consumption
- [x] Custom error decoding (e.g., `MaxSupplyExceeded`, `OwnableUnauthorizedAccount`)
- [x] Tested against Anvil and Sepolia
- [x] Progress reporting for preceding transaction replay
- [x] Quick mode (`--quick`) to skip preceding tx replay

### Phase 2: Proxy support + Etherscan

- [x] ABI selector-based fallback identification for proxy contracts
- [x] UUPS/transparent proxy resolution (tested with complex multi-proxy transaction)
- [x] Etherscan/Sourcify source fetching for external contracts
- [x] Auto-detection of chain from RPC (no `--chain` needed)
- [ ] Source location mapping (file:line:column in trace output)
- [ ] Graceful degradation for unverified contracts (show selector + raw args)

### Phase 3: Interactive TUI debugger

- [ ] Step-through TUI using ratatui (like Foundry's `forge test --debug` but for any tx)
- [ ] Source-level stepping (next line, step into, step out)
- [ ] Variable watch panel
- [ ] Breakpoints by source location

### Phase 4: Web UI

- [ ] Axum server with embedded web interface
- [ ] WebSocket API for real-time stepping
- [ ] Visual source code panel with execution highlighting

### Phase 5: Enhanced variable decoding

- [ ] Full local/state variable decoding at any execution step
- [ ] Struct, mapping, and dynamic array inspection

## Development

```bash
# Build
cargo build

# Build release
cargo build --release

# Run
cargo run --bin soldebug -- 0xTX_HASH --rpc-url http://localhost:8545

# Check (fast, no codegen)
cargo check
```

The first build takes ~60s due to the large dependency tree (revm, alloy, solc). Incremental rebuilds are fast (~7s).

## Contributing

Contributions are welcome! By opening a pull request, you agree to the [Contributor License Agreement](CLA.md).

## License

This project is licensed under the [GNU Affero General Public License v3.0](LICENSE) (AGPL-3.0-or-later).

This means you can freely use, modify, and distribute this software, but if you distribute modified versions or run them as a network service, you must make your source code available under the same license.

### Third-party licenses

soldebug depends on [Foundry](https://github.com/foundry-rs/foundry) (MIT/Apache-2.0), [revm](https://github.com/bluealloy/revm) (MIT), [alloy](https://github.com/alloy-rs/alloy) (MIT/Apache-2.0), and other open source libraries. See [NOTICE](NOTICE) for full attribution.
