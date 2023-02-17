//! Safe-sync server configs.

use std::time::Duration;

use serde::Deserialize;

/// Safe-sync server application config
#[derive(Clone)]
pub struct ServiceConfig {
    /// Server port
    pub port: u16,

    /// Metrics port
    pub metrics_port: u16,

    /// Maximum number of open mailboxes
    pub max_open_mailboxes: u32,

    /// Amount of time since last activity (send or receive a message) after which mailbox is destroyed
    pub mailbox_timeout: Duration,
}

#[derive(Deserialize)]
struct RawConfig {
    /// Server port
    #[serde(default = "default_port")]
    port: u16,

    /// Metrics port
    #[serde(default = "default_metrics_port")]
    metrics_port: u16,

    /// Maximum number of open mailboxes
    #[serde(default = "default_max_open_mailboxes")]
    max_open_mailboxes: u32,

    /// Amount of time since last activity (send or receive a message) after which mailbox is destroyed, seconds
    #[serde(default = "default_mailbox_timeout")]
    pub mailbox_timeout_sec: u64,
}

fn default_port() -> u16 {
    8080
}

fn default_metrics_port() -> u16 {
    8080
}

fn default_max_open_mailboxes() -> u32 {
    100_000_000
}

fn default_mailbox_timeout() -> u64 {
    60 // 1 minute
}

pub fn load() -> Result<ServiceConfig, anyhow::Error> {
    let raw_config = envy::from_env::<RawConfig>()?;

    let config = ServiceConfig {
        port: raw_config.port,
        metrics_port: raw_config.metrics_port,
        max_open_mailboxes: raw_config.max_open_mailboxes,
        mailbox_timeout: Duration::from_secs(raw_config.mailbox_timeout_sec),
    };

    Ok(config)
}
