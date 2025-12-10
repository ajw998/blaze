pub mod history;
pub mod index;
pub mod query;

use clap::{Parser, Subcommand};
pub use history::HistoryArgs;
pub use index::IndexArgs;
pub use query::QueryArgs;

/// Common error type for command handlers
pub type CommandResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Parser, Debug)]
#[command(
    name = "blaze",
    version,
    about = "Blaze - a fast local code search engine",
    propagate_version = true
)]
pub struct Cli {
    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Create or rebuild the index for a given root directory.
    ///
    /// Example:
    ///   blaze index /home/andrew/projects
    ///   blaze --index-dir /var/lib/blaze index --rebuild /data
    Index(IndexArgs),

    /// Execute a search query against the index.
    ///
    /// Example:
    ///   blaze query 'ext:rs mmap'
    ///   blaze query -n 20 'name:Cargo.toml'
    Query(QueryArgs),

    /// Show past queries.
    History(HistoryArgs),
}
