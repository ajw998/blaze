use std::fs;
use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::Context;
use blaze_protocol::codec::{read_message, write_message};
use blaze_protocol::{DaemonRequest, DaemonResponse};
use log::{debug, error, info};
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::flag;

use crate::query::execute_query;
use crate::state::DaemonState;

/// RAII guard that ensures the Unix socket file is removed on shutdown,
/// even if we return early or panic.
struct SocketGuard<'a> {
    path: &'a Path,
}

impl<'a> Drop for SocketGuard<'a> {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_file(self.path) {
            if err.kind() != io::ErrorKind::NotFound {
                error!(
                    "Failed to remove Unix socket at {} on shutdown: {err}",
                    self.path.display()
                );
            }
        }
    }
}

pub fn run_rpc_server(state: Arc<DaemonState>) -> anyhow::Result<()> {
    let socket_path = &state.config.socket_path;

    let shutdown = Arc::new(AtomicBool::new(false));

    // Register signal handlers. They only set the atomic flag
    for sig in [SIGINT, SIGTERM] {
        flag::register(sig, Arc::clone(&shutdown))
            .with_context(|| format!("Failed to register signal handler for {sig}"))?;
    }

    // Clean up stale socket if it exists.
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

    // Ensure socket is cleaned up on any exit path.
    let _socket_guard = SocketGuard {
        path: socket_path.as_path(),
    };

    info!("blaze daemon listening on {}", socket_path.display());

    loop {
        // Fast path: if shutdown already requested, stop accepting.
        if shutdown.load(Ordering::Relaxed) {
            info!("Shutdown signal observed; stopping RPC server.");
            break;
        }

        match listener.accept() {
            Ok((stream, _addr)) => {
                let state = state.clone();
                std::thread::spawn(move || {
                    if let Err(err) = handle_client(stream, state) {
                        error!("Error while handling client: {err:#}");
                    }
                });
            }
            Err(ref err) if err.kind() == io::ErrorKind::Interrupted => {
                // System call interrupted by signal
                if shutdown.load(Ordering::Relaxed) {
                    info!("Accept interrupted by shutdown signal; exiting accept loop.");
                    break;
                }
                // Spurious EINTR... retry
                continue;
            }
            Err(err) => {
                // Non-EINTR errors: log and decide whether to break or continue.
                error!("Accept error: {err}");
                continue;
            }
        }
    }

    info!("RPC server shutdown complete.");
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
