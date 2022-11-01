//! Mailbox management

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use parking_lot::{Mutex, RwLock};
use warp::ws;

use super::client::ClientId;

/// Mailbox ID is a 30-bit unsigned integer
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct MailboxId(u32);

impl MailboxId {
    pub fn raw(&self) -> u32 {
        self.0
    }
}

#[derive(Clone, Default)]
pub struct MailboxManager {
    ids: Arc<RwLock<IdManager>>,
    mailboxes: Arc<Mutex<HashMap<MailboxId, Mailbox>>>,
}

impl MailboxManager {
    /// Create an empty mailbox with an unique ID
    pub fn create_mailbox(&self) -> MailboxId {
        let mut ids = self.ids.write();
        let id = ids.create_id();
        let mut mailboxes = self.mailboxes.lock();
        debug_assert!(!mailboxes.contains_key(&id));
        mailboxes.insert(id, Mailbox::default());
        log::trace!("{:?} created", id);
        id
    }

    /// Find an existing mailbox by ID
    pub fn find_mailbox(&self, id: u32) -> Result<MailboxId, MailboxError> {
        let id = MailboxId(id);
        let ids = self.ids.read();
        if !ids.id_exists(id) {
            return Err(MailboxError::NotFound(id));
        }
        let mailboxes = self.mailboxes.lock();
        let mailbox = mailboxes.get(&id).expect("mailbox");
        if mailbox.can_accept_connection() {
            Ok(id)
        } else {
            Err(MailboxError::Busy(id))
        }
    }

    /// Attach client to a mailbox
    pub fn attach_client(&self, mailbox_id: MailboxId, client_id: ClientId) -> Result<(), MailboxError> {
        let ids = self.ids.read();
        if !ids.id_exists(mailbox_id) {
            return Err(MailboxError::NotFound(mailbox_id));
        }
        let mut mailboxes = self.mailboxes.lock();
        let mailbox = mailboxes.get_mut(&mailbox_id).expect("mailbox");
        if !mailbox.can_accept_connection() {
            return Err(MailboxError::Busy(mailbox_id));
        }
        mailbox.attach_peer(client_id);
        log::trace!("{:?} has attached to {:?}", client_id, mailbox_id);
        Ok(())
    }

    /// Send a message to a mailbox from a specified client
    #[must_use]
    pub fn send_to_mailbox(&self, mailbox_id: MailboxId, from_client: ClientId, msg: ws::Message) -> Option<(ClientId, ws::Message)> {
        let ids = self.ids.read();
        debug_assert!(ids.id_exists(mailbox_id));
        let mut mailboxes = self.mailboxes.lock();
        let mailbox = mailboxes.get_mut(&mailbox_id).expect("mailbox");
        mailbox.send_message(from_client, msg)
    }

    /// Returns (and removes from the queue) all messages in a specified mailbox pending for a specified client
    #[must_use]
    pub fn pending_messages_for_client(&self, mailbox_id: MailboxId, for_client: ClientId) -> Vec<ws::Message> {
        let ids = self.ids.read();
        debug_assert!(ids.id_exists(mailbox_id));
        let mut mailboxes = self.mailboxes.lock();
        let mailbox = mailboxes.get_mut(&mailbox_id).expect("mailbox");
        mailbox.pending_messages(for_client)
    }

    /// Close specified mailbox for the given client.
    /// Destroys that mailbox if no more peers connected to it,
    /// otherwise list of still connected clients is returned (they must be closed externally).
    pub fn close_mailbox(&self, mailbox_id: MailboxId, for_client: ClientId) -> Vec<ClientId> {
        let mut ids = self.ids.write();
        debug_assert!(ids.id_exists(mailbox_id));
        let mut mailboxes = self.mailboxes.lock();
        let mailbox = mailboxes.get_mut(&mailbox_id).expect("mailbox");
        mailbox.detach_peer(for_client);
        log::trace!("{:?} has detached from {:?}", for_client, mailbox_id);
        if mailbox.has_connected_peers() {
            mailbox.connected_peers()
        } else {
            mailboxes.remove(&mailbox_id);
            ids.dispose_id(mailbox_id);
            log::trace!("{:?} destroyed", mailbox_id);
            Vec::default()
        }
    }
}

/// Private API, manages mailbox IDs, ensures uniqueness
#[derive(Default)]
struct IdManager {
    used_ids: HashSet<MailboxId>,
}

impl IdManager {
    fn random_id() -> MailboxId {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(1000001);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let id = id & 0x3FFFFFFF; // cut 30 bits
        MailboxId(id)
    }

    /// Create a new mailbox id that is guaranteed to be unique
    pub fn create_id(&mut self) -> MailboxId {
        let id = loop {
            let id = Self::random_id();
            if !self.used_ids.contains(&id) {
                break id;
            }
        };
        debug_assert!(!self.used_ids.contains(&id));
        self.used_ids.insert(id);
        id
    }

    /// Remove existing mailbox id
    pub fn dispose_id(&mut self, id: MailboxId) {
        debug_assert!(self.used_ids.contains(&id));
        self.used_ids.remove(&id);
    }

    /// Checks if specified ID exists
    pub fn id_exists(&self, id: MailboxId) -> bool {
        self.used_ids.contains(&id)
    }
}

/// Private API, manages peers: each mailbox can have up to 2 peers
#[derive(Default)]
struct Mailbox {
    peers: [Peer; 2],
    is_closing: bool,
}

impl Mailbox {
    /// Check if mailbox is not closed and has available slot for a peer to be attached
    /// (i.e. has less than 2 peers now)
    pub fn can_accept_connection(&self) -> bool {
        if self.is_closing {
            false
        } else {
            self.peers[0].is_free_slot() || self.peers[1].is_free_slot()
        }
    }

    /// Attach peer to this mailbox
    pub fn attach_peer(&mut self, client_id: ClientId) {
        if self.peers[0].is_free_slot() {
            self.peers[0].attach(client_id);
        } else if self.peers[1].is_free_slot() {
            self.peers[1].attach(client_id);
        } else {
            unreachable!()
        }
    }

    /// Detach peer from this mailbox
    pub fn detach_peer(&mut self, client_id: ClientId) {
        let peer = self.find_peer_mut(client_id);
        peer.detach();
        self.is_closing = true;
    }

    /// Whether this mailbox has at least one peer attached to it
    pub fn has_connected_peers(&self) -> bool {
        !self.peers[0].is_free_slot() || !self.peers[1].is_free_slot()
    }

    /// Returns the list of connected peers
    pub fn connected_peers(&self) -> Vec<ClientId> {
        self.peers.iter().filter_map(|peer| peer.client_id).collect()
    }

    /// Send message to this mailbox, using the specified client as the sender.
    /// If the receiver (the other peer in this mailbox) is not connected yet,
    /// the message is enqueued and the returned value is `None`,
    /// otherwise (if the received is connected and his ID is known) the same message
    /// is returned together with the receiver's ID, so that it can be sent to him directly.
    #[must_use]
    pub fn send_message(&mut self, src: ClientId, msg: ws::Message) -> Option<(ClientId, ws::Message)> {
        let target_peer = self.find_other_peer_mut(src);
        target_peer.enqueue_or_send_message(msg)
    }

    /// Returns enqueued messages for the specified client (and removes these from the queue)
    #[must_use]
    pub fn pending_messages(&mut self, dest: ClientId) -> Vec<ws::Message> {
        let peer = self.find_peer_mut(dest);
        peer.take_pending_messages()
    }

    fn find_peer_mut(&mut self, client_id: ClientId) -> &mut Peer {
        debug_assert!(self.has_connected_peers());
        if self.peers[0].client_id == Some(client_id) {
            &mut self.peers[0]
        } else if self.peers[1].client_id == Some(client_id) {
            &mut self.peers[1]
        } else {
            unreachable!()
        }
    }

    fn find_other_peer_mut(&mut self, client_id: ClientId) -> &mut Peer {
        debug_assert!(self.has_connected_peers());
        if self.peers[0].client_id == Some(client_id) {
            &mut self.peers[1]
        } else if self.peers[1].client_id == Some(client_id) {
            &mut self.peers[0]
        } else {
            unreachable!()
        }
    }
}

#[derive(Default)]
struct Peer {
    client_id: Option<ClientId>,
    pending_messages: Vec<ws::Message>,
}

impl Peer {
    /// Whether client id is not attached to this peer yet
    pub fn is_free_slot(&self) -> bool {
        self.client_id.is_none()
    }

    /// Attach client id to this peer
    pub fn attach(&mut self, client_id: ClientId) {
        debug_assert!(self.client_id.is_none());
        self.client_id = Some(client_id);
    }

    /// Detach client from this peer
    pub fn detach(&mut self) {
        debug_assert!(self.client_id.is_some());
        self.client_id = None;
    }

    /// Enqueue the message if the client is not attached yet,
    /// otherwise returns the same message together with the client ID
    /// so that it can be sent directly to him.
    #[must_use]
    pub fn enqueue_or_send_message(&mut self, msg: ws::Message) -> Option<(ClientId, ws::Message)> {
        if let Some(client_id) = self.client_id {
            debug_assert!(self.pending_messages.is_empty());
            Some((client_id, msg))
        } else {
            self.pending_messages.push(msg);
            None
        }
    }

    /// Take enqueued messages
    #[must_use]
    pub fn take_pending_messages(&mut self) -> Vec<ws::Message> {
        std::mem::replace(&mut self.pending_messages, Vec::new())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum MailboxError {
    #[error("not found: {0:?}")]
    NotFound(MailboxId),
    #[error("busy: {0:?} has already two peers connected")]
    Busy(MailboxId),
}
