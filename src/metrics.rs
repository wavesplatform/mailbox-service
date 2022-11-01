use lazy_static::lazy_static;
use prometheus::{Counter, IntGauge};

lazy_static! {
    pub static ref ACTIVE_CLIENTS: IntGauge =
        IntGauge::new("Active_Clients_Count", "Number of connected clients").expect("can't create Active_Clients_Count metric");
    pub static ref CLIENT_CONNECT: Counter =
        Counter::new("Client_Connected", "Client connect events").expect("can't create Client_Connected metric");
    pub static ref CLIENT_DISCONNECT: Counter =
        Counter::new("Client_Disconnected", "Client disconnect events").expect("can't create Client_Disconnected metric");
}
