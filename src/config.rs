use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub amo: AmoConfig,
    pub mkube: MkubeConfig,
    pub metadata: MetadataConfig,
    #[serde(default)]
    pub static_users: Vec<StaticUserConfig>,
    #[serde(default)]
    pub static_host_access: Vec<StaticHostAccessConfig>,
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

/// A user defined directly in config.toml (bypass AMO for bootstrap).
/// SSH keys are loaded from files referenced by `ssh_key_files`.
#[derive(Debug, Clone, Deserialize)]
pub struct StaticUserConfig {
    pub name: String,
    #[serde(default = "default_uid")]
    pub uid: u32,
    #[serde(default)]
    pub gid: u32,
    #[serde(default = "default_shell")]
    pub shell: String,
    #[serde(default)]
    pub groups: Vec<String>,
    /// Paths to .pub key files (one key per line, like authorized_keys).
    #[serde(default)]
    pub ssh_key_files: Vec<String>,
}

fn default_uid() -> u32 {
    1000
}

fn default_shell() -> String {
    "/bin/bash".to_string()
}

/// A host access rule defined directly in config.toml.
/// Use hosts = ["*"] to match all known BMH hosts.
#[derive(Debug, Clone, Deserialize)]
pub struct StaticHostAccessConfig {
    pub ssh_users: Vec<String>,
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub users: Vec<String>,
    #[serde(default)]
    pub sudo: bool,
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }
}
