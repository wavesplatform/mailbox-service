//! Websocket connections management

use std::{iter, sync::Arc};

use futures::{SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot};
use warp::ws;

use super::{super::Server, client::Client};
use crate::metrics::{ACTIVE_CLIENTS, CLIENT_CONNECT, CLIENT_DISCONNECT};

pub async fn handle_connection(mut socket: ws::WebSocket, server: Arc<Server>, shutdown_signal: mpsc::Sender<()>) {
    let (client_tx, client_rx) = mpsc::unbounded_channel();
    let (kill_tx, kill_rx) = oneshot::channel();

    let client = Client::new(client_tx, kill_tx);
    log::info!("{:?} connected", client.id);

    ACTIVE_CLIENTS.inc();
    CLIENT_CONNECT.inc();

    server.clients.add(client.clone());

    // run ws messages processing loop
    let run_handler = run(&mut socket, &client, client_rx, &server);

    tokio::select! {
        _ = run_handler => {}
        _ = shutdown_signal.closed() => {
            log::trace!("terminating {:?} due to server shutdown", client.id);
        }
        _ = kill_rx => {
            log::trace!("kill signal handled by {:?}", client.id);
        }
    }

    // close the associated mailbox (if any) and kick the other client connected to the same mailbox
    if let Some(mailbox_id) = client.mailbox_id() {
        let to_kill = server.mailbox_manager.close_mailbox(mailbox_id, client.id);
        for target_id in to_kill {
            if let Some(target) = server.clients.find(target_id) {
                log::trace!("forcibly killing {:?} because {:?} is being destroyed", target_id, mailbox_id);
                target.kill();
            }
        }
    }

    // handle connection close
    finalize_connection(socket).await;

    server.clients.remove(client.id);

    ACTIVE_CLIENTS.dec();
    CLIENT_DISCONNECT.inc();

    log::info!("{:?} disconnected", client.id);
}

async fn run(socket: &mut ws::WebSocket, client: &Client, mut client_rx: mpsc::UnboundedReceiver<ws::Message>, server: &Server) {
    loop {
        tokio::select! {
            // Incoming message (from ws)
            next_message = socket.next() => {
                if let Some(next_msg_result) = next_message {
                    let msg = match next_msg_result {
                        Ok(msg) => msg,
                        Err(disconnected_err) => {
                            log::debug!("Connection to {:?} closed: {}", client.id, disconnected_err);
                            break;
                        }
                    };

                    if msg.is_close() {
                        log::debug!("Connection to {:?} was closed by the remote side", client.id);
                        break;
                    }

                    if msg.is_ping() || msg.is_pong() {
                        continue;
                    }

                    if let Err(failed_msg) = handle_incoming_message(client, msg, server) {
                        log::trace!("Error processing {:?} message: {:?}", client.id, failed_msg);
                        log::debug!("Error occurred while sending message to {:?}", client.id);
                        break;
                    }
                }
            }

            // Outgoing message
            msg = client_rx.recv() => {
                if let Some(message) = msg {
                    log::debug!("Sending message to {:?}", client.id);
                    if let Err(err) = socket.send(message).await {
                        log::debug!("Error while sending to {:?}: {:?}", client.id, err);
                        break;
                    }
                } else {
                    break;
                }
            }
        }
    }
}

/// Handle incoming message for the given client.
/// Returns the same message in case of errors (when the message is not processed).
fn handle_incoming_message(client: &Client, msg: ws::Message, server: &Server) -> Result<(), ws::Message> {
    if let Some(mailbox_id) = client.mailbox_id() {
        let immediate_send = server.mailbox_manager.send_to_mailbox(mailbox_id, client.id, msg);
        if let Some((client_id, msg)) = immediate_send {
            if let Some(client) = server.clients.find(client_id) {
                let sent = client.send_message(msg);
                if !sent {
                    log::debug!("Send message to {:?} failed - disconnected early?", client_id);
                }
            } else {
                log::debug!(
                    "{:?} not found (disconnected early?) - failed to send message: {:?}",
                    client_id,
                    msg,
                );
            }
        }
    } else {
        let (reply_message, pending_messages) = match initial_message::Request::parse(&msg) {
            Ok(initial_message::Request::CreateMailbox) => {
                let mailbox_id = server.mailbox_manager.create_mailbox();
                client.set_mailbox_id(mailbox_id);
                server
                    .mailbox_manager
                    .attach_client(mailbox_id, client.id)
                    .expect("new mailbox failed");
                log::debug!("{:?} has created {:?}", client.id, mailbox_id);
                let reply = initial_message::Reply::Created { id: mailbox_id.raw() };
                (reply, None)
            }
            Ok(initial_message::Request::ConnectToMailbox { id }) => match server.mailbox_manager.find_mailbox(id) {
                Ok(mailbox_id) => {
                    client.set_mailbox_id(mailbox_id);
                    match server.mailbox_manager.attach_client(mailbox_id, client.id) {
                        Ok(()) => log::debug!("{:?} has connected to {:?}", client.id, mailbox_id),
                        Err(err) => log::debug!("{:?} has failed to connect to mailbox: {:?}", client.id, err),
                    }
                    let reply = initial_message::Reply::Connected { id: mailbox_id.raw() };
                    let pending = server.mailbox_manager.pending_messages_for_client(mailbox_id, client.id);
                    (reply, Some(pending))
                }
                Err(err) => {
                    log::debug!("{:?} has tried to connect to an invalid mailbox: {:?}", client.id, err);
                    return Err(msg);
                }
            },
            Err(err) => {
                log::debug!("{:?} error: {} - {:?}", client.id, err, msg);
                return Err(msg);
            }
        };
        let reply_message = reply_message.format();
        for msg in iter::once(reply_message).chain(pending_messages.unwrap_or_default()) {
            let sent = client.send_message(msg);
            if !sent {
                log::debug!("Send reply message to {:?} failed - disconnected early?", client.id);
            }
        }
    }

    Ok(())
}

mod initial_message {
    use serde::{Deserialize, Serialize};
    use warp::ws;

    #[derive(Debug, Deserialize)]
    #[serde(tag = "req")]
    pub(super) enum Request {
        /// 'Create a nex mailbox' message
        #[serde(rename = "create")]
        CreateMailbox,

        /// 'Connect to an existing mailbox' message
        #[serde(rename = "connect")]
        ConnectToMailbox { id: u32 },
    }

    impl Request {
        pub(super) fn parse(msg: &ws::Message) -> Result<Request, Error> {
            let msg = msg.as_bytes();
            serde_json::from_slice(msg).map_err(|e| match e.classify() {
                serde_json::error::Category::Data => Error::UnrecognizedInitialMessage(e.to_string()),
                _ => Error::ErrorParsingJson(e),
            })
        }
    }

    #[derive(Clone, Debug, Serialize)]
    #[serde(tag = "resp")]
    pub enum Reply {
        /// 'Mailbox successfully created' message
        #[serde(rename = "created")]
        Created {
            #[serde(rename = "id")]
            id: u32,
        },

        /// 'Successfully connected to mailbox' message
        #[serde(rename = "connected")]
        Connected {
            #[serde(rename = "id")]
            id: u32,
        },
    }

    impl Reply {
        pub(super) fn format(self) -> ws::Message {
            let json = serde_json::to_string(&self).expect("format json failed");
            ws::Message::text(&json)
        }
    }

    #[derive(thiserror::Error, Debug)]
    pub(super) enum Error {
        #[error("failed to parse initial message as JSON: {0}")]
        ErrorParsingJson(#[from] serde_json::Error),
        #[error("unrecognized initial message: {0}")]
        UnrecognizedInitialMessage(String),
    }
}

async fn finalize_connection(mut socket: ws::WebSocket) {
    // Can safely ignore errors here because this is the final message before socket closing
    let _ = socket.send(ws::Message::close_with(1000u16, "")).await;
    let _ = socket.close().await;
}
