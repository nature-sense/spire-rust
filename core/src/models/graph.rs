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

use crate::models::memory_graph::{GraphEdge, GraphNode};

/// A query against the knowledge graph.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQuery {
    pub query_type: GraphQueryType,
    pub node_id: Option<String>,
    pub node_label: Option<String>,
    pub max_depth: Option<usize>,
    pub limit: Option<usize>,
}

/// The type of graph query to perform.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GraphQueryType {
    /// Find neighbors of a node
    Neighbors,
    /// Find paths between two nodes
    Path,
    /// Search nodes by label
    Search,
    /// Get subgraph around a node
    Subgraph,
}

/// The result of a graph query.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphResult {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub total_count: usize,
}
