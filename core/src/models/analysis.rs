use serde::{Deserialize, Serialize};

/// A request to analyze a piece of code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeAnalysisRequest {
    pub code: String,
    pub language: String,
    pub file_path: Option<String>,
}

/// The result of a code analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeAnalysis {
    pub summary: String,
    pub complexity: Option<ComplexityScore>,
    pub symbols: Vec<SymbolInfo>,
    pub suggestions: Vec<String>,
}

/// Complexity scoring for analyzed code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityScore {
    pub cyclomatic: u32,
    pub cognitive: u32,
    pub lines_of_code: u32,
}

/// Information about a symbol found in code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub line: u32,
    pub column: u32,
}

/// The kind of a code symbol.
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub max_results: Option<usize>,
    pub file_pattern: Option<String>,
}
