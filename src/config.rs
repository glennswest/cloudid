use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub amo: AmoConfig,
    pub mkube: MkubeConfig,
    pub metadata: MetadataConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub metadata_addr: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AmoConfig {
    pub nats_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MkubeConfig {
    pub url: String,
    pub bmh_namespaces: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetadataConfig {
    pub domain_suffix: String,
    pub availability_zone: String,
    #[serde(default = "default_cache_interval")]
    pub cache_rebuild_interval_secs: u64,
    #[serde(default)]
    pub dhcp_sources: Vec<String>,
}

fn default_cache_interval() -> u64 {
    30
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }
}
