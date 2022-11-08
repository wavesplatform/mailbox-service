//! Safe-sync server configs.

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

pub fn load() -> Result<ServiceConfig, anyhow::Error> {
    let raw_config = envy::from_env::<RawConfig>()?;

    let config = ServiceConfig {
        port: raw_config.port,
        metrics_port: raw_config.metrics_port,
        max_open_mailboxes: raw_config.max_open_mailboxes,
    };

    Ok(config)
}
