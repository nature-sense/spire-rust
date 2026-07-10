# Spire Packaging Structure

This document describes the packaging structure for the Spire project, including the VS Code extension (`spire-extension/`), the Rust core (`spire-core/`), and the Rust MCP servers (`mcp/`).

---

## Table of Contents

1. [Directory Layout](#directory-layout)
2. [Cargo Workspace](#cargo-workspace)
3. [Build Pipeline](#build-pipeline)
4. [Binary Distribution](#binary-distribution)
5. [VSIX Packaging](#vsix-packaging)
6. [Development Workflow](#development-workflow)
7. [CI/CD Integration](#cicd-integration)

---

## Directory Layout

```
spire-rust/
│
├── Cargo.toml                  # ← Cargo workspace root
├── Cargo.lock                  # ← Single lockfile for all Rust crates
├── target/                     # ← Shared build output directory
│
├── spire-core/                 # Rust actor system + MCP client
│   ├── Cargo.toml              # Workspace member
│   ├── src/
│   │   ├── main.rs             # Entry point
│   │   ├── lib.rs              # Crate root
│   │   ├── framework/          # Actor framework
│   │   ├── actors/             # Actor implementations
│   │   ├── mcp/                # MCP protocol layer
│   │   └── transport/          # stdio transport
│   └── tests/                  # Integration tests
│
├── mcp/                        # MCP server implementations
│   ├── mcp-git/                # Git operations MCP server
│   │   ├── Cargo.toml          # Workspace member
│   │   └── src/
│   │       ├── main.rs         # MCP server entry point
│   │       └── git_ops.rs      # Git operations implementation
│   ├── mcp-process/            # Process management MCP server
│   │   ├── Cargo.toml          # Workspace member
│   │   └── src/
│   │       ├── main.rs         # MCP server entry point
│   │       └── process_manager.rs  # Process management implementation
│   └── mcp-search/             # Content search MCP server
│       ├── Cargo.toml          # Workspace member
│       └── src/
│           ├── main.rs         # MCP server entry point
│           └── search_engine.rs    # Search engine implementation
│
├── spire-extension/            # VS Code extension (TypeScript)
│   ├── bin/                    # ← Pre-compiled Rust binaries (per platform)
│   │   ├── darwin-arm64/
│   │   │   ├── spire-core
│   │   │   ├── mcp-git
│   │   │   ├── mcp-process
│   │   │   └── mcp-search
│   │   ├── darwin-x64/
│   │   ├── linux-x64/
│   │   └── win32-x64/
│   ├── src/                    # TypeScript source
│   ├── dist/                   # Compiled extension output
│   └── package.json            # VS Code extension manifest
│
├── scripts/
│   └── stage-binaries.mjs      # Binary staging script for VSIX packaging
│
├── doc/                        # Documentation
│   ├── packaging-structure.md  # ← THIS DOCUMENT
│   ├── extension-core-interface.md
│   ├── messages-and-types.md
│   ├── actors-and-messages.md
│   ├── graph-schema.md
│   ├── agent-infrastructure.md
│   └── test-suite-reference.md
│
├── package.json                # Root package.json (build orchestration)
├── pnpm-workspace.yaml         # pnpm workspace (spire-extension only)
├── .gitignore
└── README.md
```

---

## Cargo Workspace

### Root `Cargo.toml`

The root `Cargo.toml` defines a Cargo workspace that includes all Rust crates:

```toml
[workspace]
resolver = "2"
members = [
    "spire-core",
    "mcp/mcp-git",
    "mcp/mcp-process",
    "mcp/mcp-search",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "GPL-3.0-or-later"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rust-mcp-sdk = "0.10"
rust-mcp-transport = "0.9"
rust-mcp-schema = "0.10"
anyhow = "1"
thiserror = "2"
async-trait = "0.1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
uuid = { version = "1", features = ["v4"] }
```

### Workspace Inheritance

Each crate's `Cargo.toml` uses workspace inheritance to avoid duplicating version information:

```toml
[package]
name = "mcp-git"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
tokio = { workspace = true }
serde = { workspace = true }
# ... local-only deps
git2 = "0.19"
```

### Benefits

- **Single `cargo build --release --workspace`** compiles all crates
- **Single `cargo test --workspace`** runs all 65+ tests
- **Shared dependency resolution** — dependencies are compiled once and reused across crates
- **Centralized version management** — all shared dependency versions in one place
- **Single `Cargo.lock`** ensures consistent builds

---

## Build Pipeline

### Scripts (`package.json`)

```json
{
  "scripts": {
    "build:rust": "cargo build --release --workspace",
    "build:extension": "cd spire-extension && node esbuild.config.mjs",
    "build": "pnpm run build:rust && pnpm run build:extension",
    "stage:binaries": "node scripts/stage-binaries.mjs",
    "dev": "pnpm run build && pnpm run stage:binaries && code --extensionDevelopmentPath=./spire-extension --disable-extensions",
    "test:rust": "cargo test --workspace",
    "test:extension": "cd spire-extension && npm test",
    "test": "pnpm run test:rust && pnpm run test:extension",
    "package": "pnpm run build && pnpm run stage:binaries && cd spire-extension && vsce package"
  }
}
```

### Build Order

1. **`build:rust`** — Compiles all Rust crates in release mode
2. **`build:extension`** — Compiles the TypeScript extension with esbuild
3. **`stage:binaries`** — Copies compiled binaries to `spire-extension/bin/<platform>/`

---

## Binary Distribution

### Platform Detection

The `scripts/stage-binaries.mjs` script detects the current platform and copies binaries to the correct subdirectory:

| Platform | Directory |
|----------|-----------|
| macOS ARM64 | `spire-extension/bin/darwin-arm64/` |
| macOS x64 | `spire-extension/bin/darwin-x64/` |
| Linux x64 | `spire-extension/bin/linux-x64/` |
| Windows x64 | `spire-extension/bin/win32-x64/` |

### Binaries Staged

| Binary | Source Crate | Description |
|--------|-------------|-------------|
| `spire-core` | `spire-core/` | Main Rust core process |
| `mcp-git` | `mcp/mcp-git/` | Git operations MCP server |
| `mcp-process` | `mcp/mcp-process/` | Process management MCP server |
| `mcp-search` | `mcp/mcp-search/` | Content search MCP server |

### Binary Resolution (Extension Side)

The VS Code extension resolves the Rust binary path using this priority:

```typescript
function resolveBinaryPath(name: string): string {
  const platform = `${process.platform}-${process.arch}`;
  const ext = process.platform === 'win32' ? '.exe' : '';
  // 1. Extension's bundled bin directory
  const bundled = path.join(__dirname, '..', 'bin', platform, `${name}${ext}`);
  if (fs.existsSync(bundled)) return bundled;
  // 2. Absolute path from config
  // 3. PATH lookup
}
```

---

## VSIX Packaging

### `.vscodeignore`

The `.vscodeignore` file excludes source files but includes the pre-compiled binaries:

```
.vscode/**
.vscode-test/**
src/**
node_modules/**
test/**
tsconfig.json
esbuild.config.mjs
.gitignore
**/*.ts
**/*.map
```

The `bin/` directory is **not** excluded, so platform-specific binaries are bundled into the `.vsix` package.

### Packaging Command

```bash
pnpm run package
```

This runs:
1. `cargo build --release --workspace` — compile all Rust crates
2. `node esbuild.config.mjs` — compile TypeScript extension
3. `node scripts/stage-binaries.mjs` — copy binaries to `spire-extension/bin/`
4. `vsce package` — create the `.vsix` file

---

## Development Workflow

### Quick Start

```bash
# Build everything and launch VS Code with the extension
pnpm run dev
```

### Running Tests

```bash
# Run all Rust tests
cargo test --workspace

# Run all TypeScript tests
cd spire-extension && npm test

# Run all tests
pnpm run test
```

### Building Only

```bash
# Build Rust crates only
cargo build --release --workspace

# Build extension only
cd spire-extension && node esbuild.config.mjs

# Build everything
pnpm run build
```

### Staging Binaries

```bash
# After building Rust crates, stage binaries for VSIX packaging
node scripts/stage-binaries.mjs
```

---

## CI/CD Integration

### GitHub Actions (Recommended)

```yaml
jobs:
  build:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions-rust-lang/setup-rust-toolchain@v1
      - uses: pnpm/action-setup@v2
      - run: pnpm install
      - run: pnpm run build
      - run: pnpm run test
      - run: pnpm run package
      - uses: actions/upload-artifact@v4
        with:
          name: spire-${{ matrix.os }}
          path: spire-extension/*.vsix
```

### Cross-Compilation

For cross-platform builds, use `rustup target add` to add the target architecture:

```bash
# macOS ARM64 (native on Apple Silicon)
cargo build --release --workspace --target aarch64-apple-darwin

# macOS x64 (Intel)
cargo build --release --workspace --target x86_64-apple-darwin

# Linux x64
cargo build --release --workspace --target x86_64-unknown-linux-gnu

# Windows x64
cargo build --release --workspace --target x86_64-pc-windows-msvc
```

---

## Related

- [Root README](../README.md) — Project overview and quick start
- [Extension-Core Interface](extension-core-interface.md) — Communication protocol
- [Messages and Types](messages-and-types.md) — Actor message reference
- [Test Suite Reference](test-suite-reference.md) — Test documentation
