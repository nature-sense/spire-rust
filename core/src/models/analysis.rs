// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use serde::{Deserialize, Serialize};

/// A request to analyze a piece of code.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeAnalysisRequest {
    pub code: String,
    pub language: String,
    pub file_path: Option<String>,
}

/// The result of a code analysis.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeAnalysis {
    pub summary: String,
    pub complexity: Option<ComplexityScore>,
    pub symbols: Vec<SymbolInfo>,
    pub suggestions: Vec<String>,
}

/// Complexity scoring for analyzed code.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityScore {
    pub cyclomatic: u32,
    pub cognitive: u32,
    pub lines_of_code: u32,
}

/// Information about a symbol found in code.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub line: u32,
    pub column: u32,
}

/// The kind of a code symbol.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Class,
    Variable,
    Method,
    Interface,
    Enum,
    Struct,
    Trait,
    Module,
    Unknown,
}

/// A search result from the codebase.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub file_path: String,
    pub line: u32,
    pub column: u32,
    pub snippet: String,
    pub score: f64,
    pub context: Option<String>,
}

/// A request to search the codebase.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub max_results: Option<usize>,
    pub file_pattern: Option<String>,
}
