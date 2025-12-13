use serde::Deserialize;
use std::path::PathBuf;
use std::fs;
use anyhow::Context;

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub listen_address: String,
    pub listen_port: u16,
    pub dhcp_lease_file: PathBuf,
    pub hosts_file: PathBuf,
    pub domain_suffix: String,
}

impl Config {
    pub fn load(path: &PathBuf) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {:?}", path))?;
        let config: Config = toml::from_str(&content)
            .context("Failed to parse config file")?;
        Ok(config)
    }
}
