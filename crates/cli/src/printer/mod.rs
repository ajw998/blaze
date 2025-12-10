use blaze_engine::PipelineMetrics;
use std::io::{self, Write};

/// Trait for writing status messages (daemon, indexing progress, etc).
pub trait StatusWriter {
    fn write_status(&mut self, msg: &str) -> io::Result<()>;
}

/// Default status writer that outputs to stderr.
pub struct StderrWriter;

impl StatusWriter for StderrWriter {
    fn write_status(&mut self, msg: &str) -> io::Result<()> {
        eprintln!("{}", msg);
        Ok(())
    }
}

/// Buffering status writer for testing.
#[derive(Default)]
pub struct BufferedWriter {
    buf: Vec<String>,
}

impl BufferedWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lines(&self) -> &[String] {
        &self.buf
    }
}

impl StatusWriter for BufferedWriter {
    fn write_status(&mut self, msg: &str) -> io::Result<()> {
        self.buf.push(msg.to_owned());
        Ok(())
    }
}

/// Terse helper macro for writing status messages.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        #[allow(unused_imports)]
        use $crate::core::printer::{StatusWriter as _, StderrWriter};
        let _ = StderrWriter.write_status(&format!($($arg)*));
    }};
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable output with optional colors.
    #[default]
    Human,
    /// NDJSON (newline-delimited JSON) for machine consumption.
    Json,
}

/// Color handling strategy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorChoice {
    /// Automatically detect TTY and enable colors if appropriate.
    #[default]
    Auto,
    /// Always use colors.
    Always,
    /// Never use colors.
    Never,
}

/// Configuration for printing query results.
#[derive(Debug, Clone)]
pub struct PrinterConfig {
    /// Output format (human or JSON).
    pub format: OutputFormat,
    /// Color handling strategy.
    pub color: ColorChoice,
    /// Maximum number of results to print.
    pub limit: usize,
    /// Whether to show timing statistics.
    pub show_timing: bool,
}

impl Default for PrinterConfig {
    fn default() -> Self {
        Self {
            format: OutputFormat::Human,
            color: ColorChoice::Auto,
            limit: 100,
            show_timing: true,
        }
    }
}

/// Human-readable printer with optional color support.
pub struct HumanPrinter<W: Write, E: Write> {
    out: W,
    err: E,
    cfg: PrinterConfig,
    use_color: bool,
}

impl<W: Write, E: Write> HumanPrinter<W, E> {
    pub fn new(out: W, err: E, cfg: PrinterConfig) -> Self {
        let use_color = match cfg.color {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => {
                // Use std::io::IsTerminal for TTY detection
                // Since we're generic over W, we can't check directly.
                // The caller should set ColorChoice::Always/Never explicitly
                // if they need precise control, or we default to no color
                // when Auto is used with non-stdout writers.
                false
            }
        };

        Self {
            out,
            err,
            cfg,
            use_color,
        }
    }

    /// Create a printer that writes to stdout and stderr with TTY detection.
    pub fn stdout(cfg: PrinterConfig) -> HumanPrinter<io::Stdout, io::Stderr> {
        use std::io::IsTerminal;

        let use_color = match cfg.color {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => io::stdout().is_terminal(),
        };

        HumanPrinter {
            out: io::stdout(),
            err: io::stderr(),
            cfg,
            use_color,
        }
    }

    #[inline]
    fn format_path(&self, path: &str) -> String {
        if self.use_color {
            format!("\x1b[32m{}\x1b[0m", path)
        } else {
            path.to_owned()
        }
    }
}

pub struct JsonPrinter<W: Write, E: Write> {
    out: W,
    err: E,
    cfg: PrinterConfig,
}

impl<W: Write, E: Write> JsonPrinter<W, E> {
    pub fn new(out: W, err: E, cfg: PrinterConfig) -> Self {
        Self { out, err, cfg }
    }

    /// Create a printer that writes to stdout and stderr.
    pub fn stdout(cfg: PrinterConfig) -> JsonPrinter<io::Stdout, io::Stderr> {
        JsonPrinter {
            out: io::stdout(),
            err: io::stderr(),
            cfg,
        }
    }
}

/// Static context about a print run.
#[derive(Debug)]
pub struct QueryPrintContext<'a> {
    /// Label for this query type
    pub kind: &'a str,
    /// Original query string, if available.
    pub query: Option<&'a str>,
    /// Total number of results (before limit)
    pub total: usize,
    /// Whether output was truncated due to limit.
    pub truncated: bool,
    /// Optional timing metrics.
    pub metrics: Option<&'a PipelineMetrics>,
}

/// One row in the result stream.
///
/// This struct is intentionally minimal and generic, allowing future
/// extension with fields like `line`, `column`, `snippet`, `score`.
#[derive(Debug)]
pub struct QueryRow<'a> {
    /// 1-based rank of this result.
    pub rank: usize,
    /// Full path to the file.
    pub path: &'a str,
}

// QueryPrinter trait
/// Trait for printing query results.
///
/// Implementations receive a stream of rows and context, and are responsible
/// for formatting and outputting them appropriately.
pub trait QueryPrinter {
    /// Called once before any rows are printed.
    ///
    /// Use this for headers or initial setup.
    fn begin(&mut self, ctx: &QueryPrintContext) -> io::Result<()>;

    /// Called for each result row.
    fn print_row(&mut self, row: &QueryRow<'_>, ctx: &QueryPrintContext) -> io::Result<()>;

    /// Called once after all rows are printed.
    ///
    /// Use this for footers, summaries, and timing information.
    fn finish(&mut self, ctx: &QueryPrintContext) -> io::Result<()>;
}

impl<W: Write, E: Write> QueryPrinter for HumanPrinter<W, E> {
    fn begin(&mut self, _ctx: &QueryPrintContext) -> io::Result<()> {
        Ok(())
    }

    fn print_row(&mut self, row: &QueryRow<'_>, _ctx: &QueryPrintContext) -> io::Result<()> {
        let path = self.format_path(row.path);
        writeln!(self.out, "{}", path)
    }

    fn finish(&mut self, ctx: &QueryPrintContext) -> io::Result<()> {
        if ctx.truncated {
            let remaining = ctx.total.saturating_sub(self.cfg.limit);
            writeln!(self.out, "... and {} more results", remaining)?;
        }

        if self.cfg.show_timing
            && let Some(m) = ctx.metrics
        {
            let total = m.total();
            let exec = m.exec_time.unwrap_or_default();
            let rank = m.rank_time.unwrap_or_default();

            writeln!(
                self.err,
                "\n[{}] {} results in {:.2}ms (exec: {:.2}ms, rank: {:.2}ms)",
                ctx.kind,
                ctx.total,
                total.as_secs_f64() * 1000.0,
                exec.as_secs_f64() * 1000.0,
                rank.as_secs_f64() * 1000.0,
            )?;
        }

        Ok(())
    }
}

impl<W: Write, E: Write> QueryPrinter for JsonPrinter<W, E> {
    fn begin(&mut self, _ctx: &QueryPrintContext) -> io::Result<()> {
        Ok(())
    }

    fn print_row(&mut self, row: &QueryRow<'_>, ctx: &QueryPrintContext) -> io::Result<()> {
        let obj = serde_json::json!({
            "kind": ctx.kind,
            "query": ctx.query,
            "rank": row.rank,
            "path": row.path,
        });
        writeln!(self.out, "{}", obj)
    }

    fn finish(&mut self, ctx: &QueryPrintContext) -> io::Result<()> {
        if self.cfg.show_timing
            && let Some(m) = ctx.metrics
        {
            let obj = serde_json::json!({
                "type": "summary",
                "kind": ctx.kind,
                "query": ctx.query,
                "total": ctx.total,
                "truncated": ctx.truncated,
                "timing_ms": {
                    "total": m.total().as_secs_f64() * 1000.0,
                    "exec": m.exec_time.unwrap_or_default().as_secs_f64() * 1000.0,
                    "rank": m.rank_time.unwrap_or_default().as_secs_f64() * 1000.0,
                }
            });
            writeln!(self.err, "{}", obj)?;
        }

        Ok(())
    }
}
