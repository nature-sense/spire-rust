// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Project Analyzer — standalone library for analysing project directory structure.
//!
//! This module provides the building blocks for project analysis:
//!
//! 1. **Scanner** — Walk the filesystem, respecting `.gitignore`, collecting `FileInfo`.
//! 2. **Build parsers** — Parse build config files (Cargo.toml, package.json, etc.)
//!    into normalized `BuildMetadata`.
//! 3. **Tree builder** — Assemble the flat file list into a hierarchical
//!    `DirectoryNode` tree with language detection, role classification, and
//!    line estimation.
//! 4. **Rust analyzer** — Run `cargo metadata` for rich Rust project metadata.
//!
//! The top-level orchestration is handled by the `ProjectAnalyzerActor`, which
//! delegates build analysis to external MCP servers. The functions in this module
//! are used as building blocks by both `ProjectAnalyzerActor` and `ProjectSyncActor`.

pub mod models;
pub mod scanner;
pub mod tree_builder;
pub mod rust_analyzer;
pub mod build_parsers;

pub use models::*;
pub use scanner::*;
pub use tree_builder::*;
