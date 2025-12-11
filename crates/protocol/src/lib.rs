pub mod codec;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryRequest {
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryMetrics {
    /// Total end-to-end time in milliseconds
    pub total_ms: f64,
    /// Time spent in the core execution
    pub exec_ms: f64,
    /// Time spent in ranking / scoring.
    pub rank_ms: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryHit {
    pub rank: u32,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryResponse {
    pub hits: Vec<QueryHit>,
    pub total: u32,
    pub metrics: Option<QueryMetrics>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DaemonRequest {
    Query(QueryRequest),
    Ping,
    Status,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DaemonResponse {
    QueryResult(QueryResponse),
    Pong,
    Status(String),
    Error(String),
}
