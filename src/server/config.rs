//! Safe-sync server configs.

use serde::Deserialize;

/// Safe-sync server application config
#[derive(Clone)]
pub struct ServiceConfig {
    /// Server port
    pub port: u16,

    /// Metrics port
    pub metrics_port: u16,
}

#[derive(Deserialize)]
struct RawConfig {
    /// Server port
    #[serde(default = "default_port")]
    port: u16,

    /// Metrics port
    #[serde(default = "default_metrics_port")]
    metrics_port: u16,
}

fn default_port() -> u16 {
    8080
}

fn default_metrics_port() -> u16 {
    8080
}

pub fn load() -> Result<ServiceConfig, anyhow::Error> {
    let raw_config = envy::from_env::<RawConfig>()?;

    let config = ServiceConfig {
        port: raw_config.port,
        metrics_port: raw_config.metrics_port,
    };

    Ok(config)
}
