use std::process::ExitCode;

use blaze_runtime::history::HistoryStore;
use clap::Args;
use log::{error, info};

#[derive(Debug, Args)]
pub struct HistoryArgs {
    /// Number of entries to display
    #[arg(long, short = 'n', default_value = "20")]
    pub limit: usize,

    /// Clear all history
    #[arg(long)]
    pub clear: bool,
}

pub fn run(args: HistoryArgs) -> ExitCode {
    let store = match HistoryStore::new() {
        Some(s) => s,
        None => {
            info!("[info] History is curently disabled");
            return ExitCode::from(0);
        }
    };

    if args.clear {
        match store.clear() {
            Ok(_) => {
                println!("History cleared");
                return ExitCode::from(0);
            }
            Err(e) => {
                error!("[error] Failed to clear history: {}", e);
                return ExitCode::from(1);
            }
        }
    }

    let queries = store.recent_queries(args.limit);

    if queries.is_empty() {
        println!("No history yet.");
        return ExitCode::from(0);
    }

    // Print header
    println!("{:<20}  {:>6}  {:>8}  QUERY", "TIMESTAMP", "HITS", "TIME");
    println!("{}", "-".repeat(72));

    for query in queries {
        let ts = query.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();

        println!(
            "{:<20}  {:>6}  {:>6}ms  {}",
            ts, query.hits, query.duration_ms, query.raw_query
        );
    }

    let total = store.count();
    if total > args.limit {
        println!(
            "\n({} more entries, use --limit to show more)",
            total - args.limit
        );
    }

    ExitCode::from(0)
}
