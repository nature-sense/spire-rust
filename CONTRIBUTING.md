# Contributing to Spire Rust

Thank you for your interest in contributing to Spire Rust! This document provides guidelines and instructions for contributing.

## Code of Conduct

By participating in this project, you agree to maintain a respectful and inclusive environment for everyone.

## How to Contribute

### Reporting Bugs

1. **Check existing issues** — Search the [issue tracker](https://github.com/naturesense/spire-rust/issues) to see if the bug has already been reported.
2. **Create a new issue** — Use the bug report template and include:
   - A clear description of the bug
   - Steps to reproduce
   - Expected vs. actual behavior
   - Environment details (OS, Rust version, VS Code version)
   - Logs or error messages (if applicable)

### Suggesting Features

1. **Open a feature request** — Use the feature request template and describe:
   - The problem you're trying to solve
   - The proposed solution
   - Any alternatives you've considered

### Pull Requests

1. **Fork the repository** and create a feature branch from `main`.
2. **Follow the coding conventions** (see below).
3. **Write tests** for new functionality.
4. **Ensure all tests pass** before submitting.
5. **Update documentation** as needed (README files, doc comments).
6. **Keep PRs focused** — one feature or fix per pull request.

## Development Setup

### Prerequisites

- **Rust** 1.75+ (stable)
- **Node.js** 20+
- **pnpm** (recommended) or npm
- **VS Code** 1.90+

### Getting Started

```bash
# Clone your fork
git clone https://github.com/your-username/spire-rust.git
cd spire-rust

# Install dependencies
pnpm install

# Build everything
pnpm run build

# Run tests
pnpm run test
```

### Development Workflow

```bash
# Build and launch VS Code debug session
pnpm run dev

# Build only the Rust core (faster iteration)
cd rust/spire-core && cargo build

# Run Rust tests
cd rust/spire-core && cargo test

# Run Rust tests with logging
RUST_LOG=spire_core=debug cargo run
```

## Coding Conventions

### Rust

- **Formatting** — Use `rustfmt` with the project's default configuration.
- **Linting** — Run `cargo clippy` before committing.
- **Naming** — Follow Rust conventions:
  - `snake_case` for functions, methods, variables, modules
  - `CamelCase` for types, traits, enums
  - `SCREAMING_SNAKE_CASE` for constants
- **Error handling** — Use `anyhow::Result` for fallible functions; use `thiserror` for typed errors.
- **Documentation** — All public items must have doc comments (`///`). Include examples where appropriate.
- **Unsafe code** — Avoid `unsafe` unless absolutely necessary. Document safety invariants.

### TypeScript

- **Formatting** — Use the project's TypeScript configuration.
- **Naming** — Follow TypeScript conventions:
  - `camelCase` for variables, functions, methods
  - `PascalCase` for classes, types, interfaces
- **Error handling** — Use typed errors; prefer `async/await` over raw promises.

### Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`
Scopes: `core`, `extension`, `doc`, `deps`

Examples:
```
feat(core): add vector search by label
fix(extension): handle process exit gracefully
docs: update architecture diagram
```

## Testing

### Rust Tests

```bash
# Run all tests (excluding model download tests)
cd rust/spire-core && cargo test

# Run embedding tests (requires model download, ~85 MB)
cd rust/spire-core && cargo test -- --ignored

# Run with test name filter
cd rust/spire-core && cargo test embedder

# Run all workspace tests
cd rust && cargo test --workspace
```

### TypeScript Tests

```bash
cd ts/spire-extension && npm run test
```

### Integration Tests

```bash
# From the project root
pnpm run test
```

## Project Structure

```
spire-rust/
├── rust/                       # All Rust crates
│   ├── spire-core/                 # Rust core engine (subprocess)
│   │   ├── src/
│   │   │   ├── main.rs                # Entry point: TCP socket + actor system
│   │   │   ├── lib.rs                 # Crate root
│   │   │   ├── framework/             # Actor framework (actor, system, messages)
│   │   │   ├── actors/                # Actor implementations
│   │   │   ├── mcp/                   # MCP protocol layer
│   │   │   ├── transport/             # TCP socket transport (JSON-RPC 2.0)
│   │   │   ├── graph/                 # Graph database wrapper (SeleneDB)
│   │   │   ├── embedder/              # Text embedding (Candle)
│   │   │   └── models/                # Shared data structures
│   │   └── tests/                     # Integration & actor tests
│   ├── mcp/                       # External MCP server implementations
│   │   ├── mcp-git/                  # Git operations MCP server
│   │   ├── mcp-process/              # Process management MCP server
│   │   ├── mcp-search/               # Code search MCP server
│   │   ├── mcp-terminal/             # Terminal management MCP server
│   │   └── mcp-filesystem/           # Filesystem operations MCP server
│   └── tools/                       # Development tools
│       └── project-analyzer/         # Project structure analyzer
│
├── ts/                        # All TypeScript/Node.js projects
│   └── spire-extension/           # VS Code extension (TypeScript)
│       ├── src/
│       │   ├── extension.ts           # Lifecycle: activate/deactivate
│       │   ├── client/                # Bidirectional client & environment client
│       │   ├── server/                # JSON-RPC server (router + handlers)
│       │   ├── model/                 # Type definitions & message schemas
│       │   ├── util/                  # Utilities (logger)
│       │   └── webview/               # Chat & config WebView UI
│       └── test/                      # Integration tests
│
├── scripts/                    # Build & packaging scripts
├── doc/                        # Reference documentation
└── .vscode/                    # VS Code debug & task configurations
```

## Release Process

1. Update version in `rust/spire-core/Cargo.toml` and `ts/spire-extension/package.json`.
2. Update `CHANGELOG.md`.
3. Create a tagged release on GitHub.
4. Build and publish the VS Code extension:
   ```bash
   pnpm run package
   # Upload the .vsix to the VS Code Marketplace
   ```

## Questions?

Open a [discussion](https://github.com/naturesense/spire-rust/discussions) or ask in the issue tracker.
