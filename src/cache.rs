use crate::config::Config;
use crate::model::{
    GroupResource, HostAccessResource, HostGroupResource, HostMetadata, UserResource,
};
use crate::resolve;
use dashmap::DashMap;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Identity data from AMO (users, groups, host access, host groups).
#[derive(Debug, Default)]
pub struct IdentityState {
    pub users: HashMap<String, UserResource>,
    pub groups: HashMap<String, GroupResource>,
    pub host_access: HashMap<String, HostAccessResource>,
    pub host_groups: HashMap<String, HostGroupResource>,
}

/// BMH state from mkube (IP -> hostname mappings, BMH labels).
#[derive(Debug, Default)]
pub struct BmhState {
    /// IP address -> short hostname
    pub ip_to_hostname: HashMap<IpAddr, String>,
    /// Hostname -> BMH labels (for selector matching)
    pub host_labels: HashMap<String, HashMap<String, String>>,
    /// Hostname -> namespace
    pub host_namespace: HashMap<String, String>,
}

/// Shared application state holding all caches.
pub struct AppState {
    /// Precomputed metadata cache: IP -> HostMetadata
    pub metadata_cache: DashMap<IpAddr, HostMetadata>,
    /// AMO identity data
    pub identity: RwLock<IdentityState>,
    /// mkube BMH data
    pub bmh: RwLock<BmhState>,
    /// Application config
    pub config: Config,
}

impl AppState {
    pub fn new(config: Config) -> Arc<Self> {
        Arc::new(Self {
            metadata_cache: DashMap::new(),
            identity: RwLock::new(IdentityState::default()),
            bmh: RwLock::new(BmhState::default()),
            config,
        })
    }

    /// Rebuild the entire metadata cache from current identity + BMH state.
    /// Called when either data source changes.
    pub async fn rebuild_cache(&self) {
        let identity = self.identity.read().await;
        let bmh = self.bmh.read().await;

        let old_count = self.metadata_cache.len();
        self.metadata_cache.clear();

        let mut count = 0;
        for (ip, hostname) in &bmh.ip_to_hostname {
            let labels = bmh.host_labels.get(hostname);
            match resolve::resolve_host(
                *ip,
                hostname,
                labels,
                &identity,
                &self.config.metadata,
            ) {
                Some(meta) => {
                    self.metadata_cache.insert(*ip, meta);
                    count += 1;
                }
                None => {
                    debug!(hostname, %ip, "no matching access rules for host");
                }
            }
        }

        if count != old_count {
            info!(hosts = count, previous = old_count, "metadata cache rebuilt");
        } else {
            debug!(hosts = count, "metadata cache rebuilt (no change)");
        }
    }

    /// Look up precomputed metadata for a source IP.
    pub fn get_metadata(&self, ip: &IpAddr) -> Option<HostMetadata> {
        self.metadata_cache.get(ip).map(|v| v.clone())
    }

    /// Trigger a BMH cache refresh if we get a request from an unknown IP.
    /// Returns true if the IP was unknown and a refresh should be triggered.
    pub fn is_unknown_ip(&self, ip: &IpAddr) -> bool {
        !self.metadata_cache.contains_key(ip)
    }
}
