//! Safe-sync Web server.

use std::sync::Arc;

use futures::Future;
use tokio::sync::{mpsc, oneshot};
use warp::{ws, Filter};
use wx_warp::{log::access, MetricsWarpBuilder};

use self::websocket::{client::Clients, mailbox::MailboxManager};
use crate::metrics::{ACTIVE_CLIENTS, CLIENT_CONNECT, CLIENT_DISCONNECT};

pub mod builder;
pub mod config;
mod websocket;

/// The web server
pub struct Server {
    port: u16,
    metrics_port: u16,
    mailbox_manager: MailboxManager,
    clients: Clients,
}

impl Server
where
    Self: Send + Sync + 'static,
{
    /// Start the web server.
    /// Returns the future that runs the web server and a sender that can be used to stop the server.
    /// The shutdown signal is propagated to each connection handler to terminate them all.
    pub fn start(self: Arc<Self>, shutdown_signal: mpsc::Sender<()>) -> (impl Future<Output = ()>, oneshot::Sender<()>) {
        let port = self.port;
        let metrics_port = self.metrics_port;
        let with_self = { warp::any().map(move || self.clone()) };
        let with_shutdown_signal = { warp::any().map(move || shutdown_signal.clone()) };

        let ws = warp::path("ws")
            .and(warp::path::end())
            .and(warp::ws())
            .and(with_self)
            .and(with_shutdown_signal)
            .map(|ws: ws::Ws, server: Arc<Self>, shutdown_signal| {
                let mailbox_manager = server.mailbox_manager.clone();
                let clients = server.clients.clone();
                ws.on_upgrade(move |socket| websocket::connection::handle_connection(socket, mailbox_manager, clients, shutdown_signal))
            })
            .with(warp::log::custom(access));

        // Signal to stop the server
        let (stop_tx, stop_rx) = oneshot::channel();

        let servers = MetricsWarpBuilder::new()
            .with_main_routes(ws)
            .with_main_routes_port(port)
            .with_metrics_port(metrics_port)
            .with_metric(&*ACTIVE_CLIENTS)
            .with_metric(&*CLIENT_CONNECT)
            .with_metric(&*CLIENT_DISCONNECT)
            .with_graceful_shutdown(async {
                let _ = stop_rx.await;
                log::trace!("server shutdown signal received");
            })
            .run_async();

        (servers, stop_tx)
    }

    /// Gracefully kill all connected websocket clients
    pub async fn disconnect_all_clients(&self) {
        let clients_to_kill = self.clients.all();
        let client_count = clients_to_kill.len();
        log::info!("About to kill {} connected clients", client_count);
        for client in clients_to_kill {
            log::trace!("Gracefully killing {:?}", client.id);
            client.kill();
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }
    }
}
