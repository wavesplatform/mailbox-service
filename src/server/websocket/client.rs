//! Clients management

use std::{collections::HashMap, sync::Arc};

use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot};
use warp::ws;

use super::mailbox::MailboxId;

/// Client ID, cheap to clone or copy.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ClientId(u64);

/// Client struct, cheaply cloneable.
#[derive(Clone)]
pub struct Client {
    pub id: ClientId,
    inner: Arc<Mutex<ClientInner>>,
}

struct ClientInner {
    sender: mpsc::UnboundedSender<ws::Message>,
    kill_sender: Option<oneshot::Sender<()>>,
    mailbox_id: Option<MailboxId>,
}

impl Client {
    pub fn new(sender: mpsc::UnboundedSender<ws::Message>, kill_sender: oneshot::Sender<()>) -> Self {
        let id = {
            use std::sync::atomic::{AtomicU64, Ordering};
            static COUNTER: AtomicU64 = AtomicU64::new(1);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            ClientId(id)
        };
        let inner = Arc::new(Mutex::new(ClientInner {
            sender,
            kill_sender: Some(kill_sender),
            mailbox_id: None,
        }));
        Client { id, inner }
    }

    pub fn mailbox_id(&self) -> Option<MailboxId> {
        self.inner.lock().mailbox_id
    }

    pub fn set_mailbox_id(&self, mailbox_id: MailboxId) {
        self.inner.lock().mailbox_id = Some(mailbox_id);
    }

    pub fn send_message(&self, msg: ws::Message) -> bool {
        let res = self.inner.lock().sender.send(msg);
        res.is_ok()
    }

    pub fn kill(&self) {
        if let Some(tx) = self.inner.lock().kill_sender.take() {
            let _ = tx.send(());
        }
    }
}

/// Client list, cheaply cloneable
#[derive(Clone, Default)]
pub struct Clients(Arc<Mutex<HashMap<ClientId, Client>>>);

impl Clients {
    pub fn add(&self, client: Client) {
        let Clients(clients) = self;
        let mut clients = clients.lock();
        debug_assert!(!clients.contains_key(&client.id));
        clients.insert(client.id, client);
    }

    pub fn remove(&self, id: ClientId) {
        let Clients(clients) = self;
        let mut clients = clients.lock();
        debug_assert!(clients.contains_key(&id));
        clients.remove(&id);
    }

    pub fn find(&self, id: ClientId) -> Option<Client> {
        let Clients(clients) = self;
        let clients = clients.lock();
        clients.get(&id).cloned()
    }

    pub fn all(&self) -> Vec<Client> {
        let Clients(clients) = self;
        let clients = clients.lock();
        clients.values().cloned().collect()
    }
}
