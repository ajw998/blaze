use crate::{FileId, Index, PipelineMetrics, QueryPipeline};

#[derive(Debug, Clone)]
pub struct EngineQueryHit {
    pub rank: usize,
    pub file_id: FileId,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct EngineQueryResult {
    /// Top N hits
    pub hits: Vec<EngineQueryHit>,
    /// Total logical hits after ranking and other filters
    pub total: usize,
    /// Optional pipeline metrics
    pub metrics: Option<PipelineMetrics>,
    /// Normalised query string
    pub query_str: Option<String>,
}

impl Index {
    pub fn run_query(&self, query: &str, limit: usize) -> EngineQueryResult {
        let pipeline = QueryPipeline::new_timed(self)
            .parse(query)
            .execute()
            .rank_with_limit(Some(limit));

        let total = pipeline.count();
        let metrics = pipeline.metrics().cloned();
        let query_str = pipeline.query_str().map(|s| s.to_owned());

        let mut hits = Vec::with_capacity(limit.min(total));
        for (rank, fid, path) in pipeline.iter_with_paths() {
            hits.push(EngineQueryHit {
                rank,
                file_id: fid,
                path,
            });
        }

        pipeline.log_history();

        EngineQueryResult {
            hits,
            total,
            metrics,
            query_str,
        }
    }
}
