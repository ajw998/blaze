use std::process::ExitCode;

use clap::Parser;

mod commands;
mod printer;

use blaze_runtime::logging;
use commands::Command;

#[derive(Debug, Parser)]
#[command(name = "blaze", version, about = "Blazingly Fast File Search")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

fn main() -> ExitCode {
    logging::init().ok();

    let cli = Cli::parse();
    match cli.command {
        Command::Query(args) => commands::query::run(args),
        Command::Index(args) => commands::index::run(args),
        Command::History(args) => commands::history::run(args),
    }
}
