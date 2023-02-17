//! Safe-sync Web server instance builder.

use builder_pattern::Builder;

use super::{
    config::ServiceConfig,
    websocket::{client::Clients, mailbox::MailboxManager},
    Server,
};

#[derive(Builder)]
pub struct ServerBuilder {
    #[public]
    config: ServiceConfig,
}

impl ServerBuilder {
    pub fn new_server(self) -> Server {
        Server {
            config: self.config,
            mailbox_manager: MailboxManager::default(),
            clients: Clients::default(),
        }
    }
}
