use crate::config::Config;
use crate::model::{
    BareMetalHost, GroupResource, HostAccessResource, HostGroupResource, HostMetadata,
    HostAccessSpec, HostAccessTargets, ResourceMeta, ResourceStatus, SshPublicKey, Subject,
    SubjectKind, UserResource, UserSpec,
};
use crate::resolve;
use crate::templates::{AssignmentsFile, OneshotState, TemplateStore};
use dashmap::DashMap;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// IMDSv2 token entry with expiration.
#[derive(Debug, Clone)]
pub struct ImdsToken {
    pub token: String,
    pub ip: IpAddr,
    pub ttl_secs: u32,
    pub created_at: u64,
}

impl ImdsToken {
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now > self.created_at + self.ttl_secs as u64
    }
}

/// Identity data from AMO (users, groups, host access, host groups).
#[derive(Debug, Default)]
pub struct IdentityState {
    pub users: HashMap<String, UserResource>,
    pub groups: HashMap<String, GroupResource>,
    pub host_access: HashMap<String, HostAccessResource>,
    pub host_groups: HashMap<String, HostGroupResource>,
}

/// BMH state from mkube (IP -> hostname mappings, BMH labels, full BMH data).
#[derive(Debug, Default)]
pub struct BmhState {
    /// IP address -> short hostname
    pub ip_to_hostname: HashMap<IpAddr, String>,
    /// Hostname -> BMH labels (for selector matching)
    pub host_labels: HashMap<String, HashMap<String, String>>,
    /// Hostname -> namespace
    pub host_namespace: HashMap<String, String>,
    /// Hostname -> full BareMetalHost data (for provisioning configs)
    pub hosts: HashMap<String, BareMetalHost>,
}

/// Shared application state holding all caches.
pub struct AppState {
    /// Precomputed metadata cache: IP -> HostMetadata
    pub metadata_cache: DashMap<IpAddr, HostMetadata>,
    /// IMDSv2 tokens: token string -> ImdsToken
    pub imds_tokens: DashMap<String, ImdsToken>,
    /// AMO identity data
    pub identity: RwLock<IdentityState>,
    /// mkube BMH data
    pub bmh: RwLock<BmhState>,
    /// Discovered data network namespaces (from mkube /api/v1/networks)
    pub data_namespaces: RwLock<Vec<String>>,
    /// Template store (file-based on PVC)
    pub template_store: TemplateStore,
    /// Host-to-template assignments
    pub assignments: RwLock<AssignmentsFile>,
    /// Oneshot completion state
    pub oneshot: RwLock<OneshotState>,
    /// Application config
    pub config: Config,
    /// Counter for generating unique tokens
    token_counter: std::sync::atomic::AtomicU64,
}

impl AppState {
    pub async fn new(config: Config) -> Arc<Self> {
        let identity = load_static_identity(&config);
        let count = identity.users.len();
        let rules = identity.host_access.len();

        let template_store = TemplateStore::new(&config.templates.data_dir);
        if let Err(e) = template_store.init().await {
            warn!(error = %e, "failed to initialize template store directories");
        }

        let assignments = template_store.load_assignments().await;
        let oneshot = template_store.load_oneshot().await;
        let asgn_count = assignments.assignments.len();
        let os_count = oneshot.completed.len();

        let state = Arc::new(Self {
            metadata_cache: DashMap::new(),
            imds_tokens: DashMap::new(),
            identity: RwLock::new(identity),
            bmh: RwLock::new(BmhState::default()),
            data_namespaces: RwLock::new(Vec::new()),
            template_store,
            assignments: RwLock::new(assignments),
            oneshot: RwLock::new(oneshot),
            config,
            token_counter: std::sync::atomic::AtomicU64::new(1),
        });

        if count > 0 {
            info!(users = count, rules, "loaded static identity from config");
        }
        if asgn_count > 0 || os_count > 0 {
            info!(assignments = asgn_count, oneshot_completed = os_count, "loaded template state from PVC");
        }

        state
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

    /// On-demand resolve for a source IP (fallback when cache misses during provisioning).
    pub async fn resolve_on_demand(&self, ip: &IpAddr) -> Option<HostMetadata> {
        // Check cache first
        if let Some(meta) = self.get_metadata(ip) {
            return Some(meta);
        }

        // Cache miss: try to resolve directly from current state
        let identity = self.identity.read().await;
        let bmh = self.bmh.read().await;

        if let Some(hostname) = bmh.ip_to_hostname.get(ip) {
            let labels = bmh.host_labels.get(hostname);
            resolve::resolve_host(*ip, hostname, labels, &identity, &self.config.metadata)
        } else {
            None
        }
    }

    /// Get the full BareMetalHost data for a hostname.
    pub async fn get_bmh(&self, hostname: &str) -> Option<BareMetalHost> {
        let bmh = self.bmh.read().await;
        bmh.hosts.get(hostname).cloned()
    }

    /// Generate an IMDSv2 token for a host IP.
    pub fn generate_imds_token(&self, ip: IpAddr, ttl_secs: u32) -> String {
        let seq = self
            .token_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let token = format!("cloudid-{:016x}-{:08x}", now, seq);

        self.imds_tokens.insert(
            token.clone(),
            ImdsToken {
                token: token.clone(),
                ip,
                ttl_secs,
                created_at: now,
            },
        );

        // Prune expired tokens (lazy cleanup)
        self.imds_tokens.retain(|_, t| !t.is_expired());

        token
    }

    /// Validate an IMDSv2 token. Returns the associated IP if valid.
    pub fn validate_imds_token(&self, token: &str) -> Option<IpAddr> {
        self.imds_tokens
            .get(token)
            .filter(|t| !t.is_expired())
            .map(|t| t.ip)
    }

    /// Trigger a BMH cache refresh if we get a request from an unknown IP.
    /// Returns true if the IP was unknown and a refresh should be triggered.
    pub fn is_unknown_ip(&self, ip: &IpAddr) -> bool {
        !self.metadata_cache.contains_key(ip)
    }
}

/// Convert static config entries into proper identity state entries.
fn load_static_identity(config: &Config) -> IdentityState {
    let mut state = IdentityState::default();

    // Load static users (read SSH keys from files)
    for user_cfg in &config.static_users {
        let mut ssh_keys: Vec<SshPublicKey> = Vec::new();
        for (i, path) in user_cfg.ssh_key_files.iter().enumerate() {
            match std::fs::read_to_string(path) {
                Ok(contents) => {
                    for (j, line) in contents.lines().enumerate() {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with('#') {
                            continue;
                        }
                        ssh_keys.push(SshPublicKey {
                            name: format!("file-{}-{}", i, j),
                            key: line.to_string(),
                        });
                    }
                }
                Err(e) => {
                    warn!(user = %user_cfg.name, path, error = %e, "failed to read SSH key file");
                }
            }
        }

        state.users.insert(
            user_cfg.name.clone(),
            UserResource {
                kind: "User".to_string(),
                metadata: ResourceMeta {
                    name: user_cfg.name.clone(),
                    namespace: String::new(),
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                },
                spec: UserSpec {
                    display_name: user_cfg.name.clone(),
                    email: None,
                    org: String::new(),
                    uid: user_cfg.uid,
                    gid: user_cfg.gid,
                    shell: user_cfg.shell.clone(),
                    ssh_public_keys: ssh_keys,
                    groups: user_cfg.groups.clone(),
                },
                status: Some(ResourceStatus { enabled: true }),
            },
        );
    }

    // Load static host access rules
    for (i, rule_cfg) in config.static_host_access.iter().enumerate() {
        let subjects: Vec<Subject> = rule_cfg
            .users
            .iter()
            .map(|name| Subject {
                kind: SubjectKind::User,
                name: name.clone(),
            })
            .collect();

        let rule_name = format!("static-rule-{}", i);

        state.host_access.insert(
            rule_name.clone(),
            HostAccessResource {
                kind: "HostAccess".to_string(),
                metadata: ResourceMeta {
                    name: rule_name,
                    namespace: String::new(),
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                },
                spec: HostAccessSpec {
                    subjects,
                    targets: HostAccessTargets {
                        hosts: rule_cfg.hosts.clone(),
                        host_groups: vec![],
                        host_selectors: vec![],
                    },
                    ssh_users: rule_cfg.ssh_users.clone(),
                    sudo: rule_cfg.sudo,
                },
                status: None,
            },
        );
    }

    state
}

// Re-export the type alias used by callers
pub use crate::model::Resource;
