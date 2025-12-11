use anyhow::Result;
use blaze_engine::{Index, PipelineMetrics, to_query_metrics};
use blaze_protocol::{QueryHit, QueryRequest, QueryResponse};

pub fn execute_query(index: &Index, req: &QueryRequest) -> Result<QueryResponse> {
    let limit = req.limit.unwrap_or(20) as usize;
    let result = index.run_query(&req.query, limit);

    let hits: Vec<QueryHit> = result
        .hits
        .into_iter()
        .map(|h| QueryHit {
            rank: h.rank as u32,
            path: h.path,
        })
        .collect();

    let metrics = result
        .metrics
        .map(|m: PipelineMetrics| to_query_metrics(&m));

    Ok(QueryResponse {
        hits,
        total: result.total as u32,
        metrics,
    })
}
