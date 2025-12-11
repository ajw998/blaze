use blaze_protocol::codec::{read_message, write_message};
use blaze_runtime::blaze_dir;
use std::io::{Stderr, Stdout};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

use anyhow::{Context, anyhow};
use blaze_engine::{Index, PipelineMetrics, to_query_metrics};
use blaze_protocol::{DaemonRequest, DaemonResponse, QueryRequest};
use blaze_runtime::default_index_path;
use clap::Args;

use crate::commands::CommandResult;
use crate::printer::{
    ColorChoice, HumanPrinter, JsonPrinter, OutputFormat, PrinterConfig, QueryPrintContext,
    QueryPrinter, QueryRow,
};

#[derive(Debug, Args)]
pub struct OutputOptions {
    /// Output results as NDJSON (one JSON object per line)
    #[arg(long)]
    pub json: bool,

    /// When to use colors: auto, always, never
    #[arg(long, value_name = "WHEN", default_value = "auto")]
    pub color: String,

    /// Suppress timing statistics
    #[arg(long, short = 'q')]
    pub quiet: bool,
}

impl OutputOptions {
    /// Create a printer based on the output options.
    pub fn make_printer(&self, limit: usize) -> Box<dyn QueryPrinter> {
        let format = if self.json {
            OutputFormat::Json
        } else {
            OutputFormat::Human
        };

        let color = match self.color.as_str() {
            "always" => ColorChoice::Always,
            "never" => ColorChoice::Never,
            _ => ColorChoice::Auto,
        };

        let cfg = PrinterConfig {
            format,
            color,
            limit,
            show_timing: !self.quiet,
        };

        match format {
            OutputFormat::Human => Box::new(HumanPrinter::<Stdout, Stderr>::stdout(cfg)),
            OutputFormat::Json => Box::new(JsonPrinter::<Stdout, Stderr>::stdout(cfg)),
        }
    }
}

#[derive(Debug, Args)]
pub struct QueryArgs {
    /// The query expression to execute
    pub query: String,

    /// Maximum number of results to display
    #[arg(long, short = 'n', default_value = "20")]
    pub limit: usize,

    /// Output formatting options
    #[command(flatten)]
    pub output: OutputOptions,

    /// Use the background daemon instead of querying index directly
    #[arg(long)]
    pub daemon: bool,
}

pub fn run(args: QueryArgs) -> ExitCode {
    match execute(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("[error] {e}");
            ExitCode::from(2)
        }
    }
}

fn execute(args: QueryArgs) -> CommandResult<ExitCode> {
    if args.daemon {
        execute_via_daemon(&args)
    } else {
        execute_local(args)
    }
}

/// Existing behaviour: open index and run pipeline in-process.
fn execute_local(args: QueryArgs) -> CommandResult<ExitCode> {
    let index_path = default_index_path();
    let index = Index::open(&index_path)?;

    run_local(&index, &args)?;

    Ok(ExitCode::from(0))
}

fn run_local(index: &Index, args: &QueryArgs) -> CommandResult<()> {
    let limit = args.limit;
    let result = index.run_query(&args.query, limit);

    let mut printer = args.output.make_printer(limit);

    let truncated = result.total > limit;

    let metrics = result
        .metrics
        .map(|m: PipelineMetrics| to_query_metrics(&m));

    let ctx = QueryPrintContext {
        kind: "query",
        query: result.query_str.as_deref(),
        total: result.total,
        truncated,
        metrics,
    };

    printer.begin(&ctx)?;

    for hit in &result.hits {
        let row = QueryRow {
            rank: hit.rank,
            path: &hit.path,
        };
        printer.print_row(&row, &ctx)?;
    }

    printer.finish(&ctx)?;

    Ok(())
}

/// Daemon mode: send the query over Unix socket and print the response.
fn execute_via_daemon(args: &QueryArgs) -> CommandResult<ExitCode> {
    let socket_path = blaze_dir().join("daemon.sock");

    let mut stream = UnixStream::connect(&socket_path).with_context(|| {
        format!(
            "failed to connect to blaze daemon at {}",
            socket_path.display()
        )
    })?;

    let req = DaemonRequest::Query(QueryRequest {
        query: args.query.clone(),
        limit: Some(args.limit),
    });

    write_message(&mut stream, &req)?;
    let resp: DaemonResponse = read_message(&mut stream)?;

    match resp {
        DaemonResponse::QueryResult(qr) => {
            // Reuse the existing printers.
            let mut printer = args.output.make_printer(args.limit);

            let total = qr.total as usize;
            let truncated = total > args.limit;

            let ctx = QueryPrintContext {
                kind: "query",
                query: Some(&args.query),
                total,
                truncated,
                metrics: qr.metrics,
            };

            printer.begin(&ctx)?;

            for hit in qr.hits.iter().take(args.limit) {
                let row = QueryRow {
                    rank: hit.rank as usize,
                    path: &hit.path,
                };
                printer.print_row(&row, &ctx)?;
            }

            printer.finish(&ctx)?;

            // History logging is already done in the daemon's pipeline.
            Ok(ExitCode::from(0))
        }
        DaemonResponse::Error(msg) => {
            // Treat daemon-reported error as a CLI error.
            Err(anyhow!("daemon error: {msg}").into())
        }
        other => Err(anyhow!("unexpected daemon response: {other:?}").into()),
    }
}
