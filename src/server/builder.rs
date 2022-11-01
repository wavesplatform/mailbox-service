//! Safe-sync Web server instance builder.

use builder_pattern::Builder;

use super::{
    websocket::{client::Clients, mailbox::MailboxManager},
    Server,
};

#[derive(Builder)]
pub struct ServerBuilder {
    #[public]
    port: u16,

    #[public]
    metrics_port: u16,
}

impl ServerBuilder {
    pub fn new_server(self) -> Server {
        Server {
            port: self.port,
            metrics_port: self.metrics_port,
            mailbox_manager: MailboxManager::default(),
            clients: Clients::default(),
        }
    }
}
