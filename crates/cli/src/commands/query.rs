use std::io::{Stderr, Stdout};
use std::process::ExitCode;

use blaze_engine::{Index, QueryPipeline};
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
    // Locate the index file and open it.
    //
    // `Index::open` returns an io::Result, but CommandResult is an anyhow::Result,
    // so `?` will convert the io::Error into anyhow::Error for us.
    let index_path = default_index_path();
    let index = Index::open(&index_path)?;

    run_query(&index, &args)?;

    Ok(ExitCode::from(0))
}

fn run_query(index: &Index, args: &QueryArgs) -> CommandResult<()> {
    // Build and execute the timed query pipeline.
    //
    // This gives us a fully ranked pipeline with metrics.
    let pipeline = QueryPipeline::new_timed(index)
        .parse(&args.query)
        .execute()
        .rank_with_limit(Some(args.limit));

    // Let the output options decide which printer to use.
    let mut printer = args.output.make_printer(args.limit);

    // Compute basic context for printing.
    let total = pipeline.count();
    let truncated = total > args.limit;
    let metrics = pipeline.metrics();
    let query_str = pipeline.query_str();

    let ctx = QueryPrintContext {
        kind: "query",
        query: query_str,
        total,
        truncated,
        metrics,
    };

    printer.begin(&ctx)?;

    for (rank, _fid, path) in pipeline.iter_with_paths() {
        let row = QueryRow { rank, path: &path };
        printer.print_row(&row, &ctx)?;
    }

    // Finish printing (footer / summary / timing).
    printer.finish(&ctx)?;

    pipeline.log_history();

    Ok(())
}
