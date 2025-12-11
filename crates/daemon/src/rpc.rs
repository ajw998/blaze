use std::fs;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;

use anyhow::Context;
use blaze_protocol::codec::{read_message, write_message};
use blaze_protocol::{DaemonRequest, DaemonResponse};
use log::{debug, error, info};

use crate::query::execute_query;
use crate::state::DaemonState;

pub fn run_rpc_server(state: Arc<DaemonState>) -> anyhow::Result<()> {
    let socket_path = &state.config.socket_path;

    if socket_path.exists() {
        fs::remove_file(socket_path).with_context(|| {
            format!(
                "Failed to remove existing socket at {}",
                socket_path.display()
            )
        })?;
    }

    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("Failed to bind Unix socket at {}", socket_path.display()))?;

    info!("blaze daemon listening on {}", socket_path.display());

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = state.clone();
                std::thread::spawn(move || {
                    if let Err(err) = handle_client(stream, state) {
                        error!("Error while handling client: {err:#}");
                    }
                });
            }
            Err(err) => {
                error!("Accept error: {err}");
            }
        }
    }

    Ok(())
}

fn handle_client(mut stream: UnixStream, state: Arc<DaemonState>) -> anyhow::Result<()> {
    let request: DaemonRequest =
        read_message(&mut stream).context("Failed to read DaemonRequest")?;

    debug!("Received request: {:?}", request);

    let response = match request {
        DaemonRequest::Ping => DaemonResponse::Pong,
        DaemonRequest::Status => DaemonResponse::Status(format!(
            "root={}, index={}",
            state.config.root.display(),
            state.config.index_path.display()
        )),
        DaemonRequest::Query(q) => match execute_query(&*state.current_index(), &q) {
            Ok(resp) => DaemonResponse::QueryResult(resp),
            Err(e) => DaemonResponse::Error(format!("Query failed: {e:#}")),
        },
    };

    write_message(&mut stream, &response).context("Failed to write DaemonResponse")
}
