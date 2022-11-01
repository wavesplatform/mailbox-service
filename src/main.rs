//! Safe-sync server

extern crate wavesexchange_log as log;
extern crate wavesexchange_warp as wx_warp;

use std::sync::Arc;

use tokio::{
    signal::unix::{signal, SignalKind},
    sync::{mpsc, oneshot},
};

mod metrics;
mod server;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    env_logger::init();

    // Load configs
    let config = server::config::load()?;

    // Create the web server
    use server::builder::ServerBuilder;
    let server = ServerBuilder::new()
        .port(config.port)
        .metrics_port(config.metrics_port)
        .build()
        .new_server();
    let server = Arc::new(server);

    // Run the web server
    let (shutdown_signal_tx, mut shutdown_signal_rx) = mpsc::channel(1);
    let (server_task, server_stop_tx) = server.clone().start(shutdown_signal_tx);
    let server_handle = tokio::spawn(server_task);

    // Graceful shutdown handling
    let (shutdown_start_tx, shutdown_start_rx) = oneshot::channel();
    let mut shutdown_start_tx = Some(shutdown_start_tx);
    let mut graceful_shutdown_handle = tokio::spawn(async move {
        if shutdown_start_rx.await.is_ok() {
            log::debug!("Graceful shutdown started: disconnecting all clients");
            server.disconnect_all_clients().await;
        }
    });

    let mut sigterm_stream = signal(SignalKind::terminate()).expect("sigterm stream");

    loop {
        tokio::select! {
            // On SIGTERM initiate graceful shutdown (subsequent SIGTERM will terminate server immediately)
            _ = sigterm_stream.recv() => {
                log::info!("got SIGTERM - shutting down gracefully");
                if let Some(tx) = shutdown_start_tx.take() { // first SIGTERM
                    let _ = tx.send(()); // start graceful shutdown
                } else { // subsequent SIGTERM
                    break; // terminate server immediately
                }
            }
            // On SIGINT terminate server immediately
            _ = tokio::signal::ctrl_c() => {
                log::info!("got SIGINT - terminating immediately");
                break; // terminate server
            }
            // When graceful shutdown handler finishes terminate the server
            _ = &mut graceful_shutdown_handle => {
                log::debug!("Graceful shutdown finished");
                break; // terminate server
            }
        }
    }

    // Send stop signal to all websocket connection handlers
    log::trace!("terminating ws connection handlers");
    shutdown_signal_rx.close();

    // Send stop signal to the web server
    log::trace!("terminating ws server");
    let _ = server_stop_tx.send(());
    server_handle.await?;

    log::info!("Server terminated");

    Ok(())
}
