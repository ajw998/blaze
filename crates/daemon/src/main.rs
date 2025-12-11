use std::sync::Arc;

mod config;
mod query;
mod rpc;
mod state;

use blaze_runtime::logging;
use config::DaemonConfig;
use state::DaemonState;

use log::info;

fn main() -> anyhow::Result<()> {
    logging::init().ok();

    let config = DaemonConfig::from_env()?;

    info!(
        "Starting blaze daemon: root={}, index={}, socket={}",
        config.root.display(),
        config.index_path.display(),
        config.socket_path.display(),
    );

    let state = Arc::new(DaemonState::new(config)?);
    rpc::run_rpc_server(state)
}
