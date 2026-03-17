use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// SSH public key with a name identifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshPublicKey {
    pub name: String,
    pub key: String,
}

/// User as stored in AMO's NATS KV bucket (AMO_USERS).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSpec {
    pub display_name: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub org: String,
    pub uid: u32,
    #[serde(default)]
    pub gid: u32,
    #[serde(default = "default_shell")]
    pub shell: String,
    #[serde(default)]
    pub ssh_public_keys: Vec<SshPublicKey>,
    #[serde(default)]
    pub groups: Vec<String>,
}

fn default_shell() -> String {
    "/bin/bash".to_string()
}

/// Group as stored in AMO's NATS KV bucket (AMO_GROUPS).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSpec {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub gid: u32,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub org: String,
}

/// Subject reference in a HostAccess rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subject {
    pub kind: SubjectKind,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SubjectKind {
    User,
    Group,
}

/// HostAccess targets specifying which hosts the rule applies to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostAccessTargets {
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub host_groups: Vec<String>,
    #[serde(default)]
    pub host_selectors: Vec<HashMap<String, String>>,
}

/// HostAccess as stored in AMO's NATS KV bucket (AMO_HOSTACCESS).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostAccessSpec {
    pub subjects: Vec<Subject>,
    pub targets: HostAccessTargets,
    #[serde(default)]
    pub ssh_users: Vec<String>,
    #[serde(default)]
    pub sudo: bool,
}

/// HostGroup as stored in AMO's NATS KV bucket (AMO_HOSTGROUPS).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostGroupSpec {
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

// --- Kubernetes-style resource wrappers (AMO uses this format in NATS KV) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMeta {
    pub name: String,
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    #[serde(default)]
    pub annotations: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceStatus {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource<T> {
    #[serde(default)]
    pub kind: String,
    pub metadata: ResourceMeta,
    pub spec: T,
    #[serde(default)]
    pub status: Option<ResourceStatus>,
}

pub type UserResource = Resource<UserSpec>;
pub type GroupResource = Resource<GroupSpec>;
pub type HostAccessResource = Resource<HostAccessSpec>;
pub type HostGroupResource = Resource<HostGroupSpec>;

// --- mkube BareMetalHost types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BmhMeta {
    pub name: String,
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    #[serde(default)]
    pub annotations: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BmhBmc {
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub mac: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub network: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BmhSpec {
    #[serde(default)]
    pub boot_mac_address: String,
    #[serde(default)]
    pub online: Option<bool>,
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub network: String,
    #[serde(default)]
    pub ip: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub bmc: Option<BmhBmc>,
    /// Template assignment from BMH CRD (format: "image_type/name" or just "name").
    #[serde(default)]
    pub template: Option<String>,
    /// Base Ignition v3 config JSON (from BMH CRD). CloudID merges SSH keys into this.
    #[serde(default)]
    pub ignition: Option<serde_json::Value>,
    /// Base kickstart config text (from BMH CRD). CloudID merges SSH keys into this.
    #[serde(default)]
    pub kickstart: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BmhStatus {
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub powered_on: bool,
    #[serde(default)]
    pub ip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BareMetalHost {
    pub metadata: BmhMeta,
    pub spec: BmhSpec,
    #[serde(default)]
    pub status: Option<BmhStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BareMetalHostList {
    pub items: Vec<BareMetalHost>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub object: BareMetalHost,
}

// --- DHCP lease types (from MicroDNS) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhcpLease {
    pub ip: String,
    #[serde(default)]
    pub mac: String,
    #[serde(default)]
    pub hostname: String,
}

// --- Precomputed metadata for a host ---

/// A set of SSH keys for a specific system user (e.g., root, core).
#[derive(Debug, Clone)]
pub struct PublicKeyEntry {
    pub ssh_user: String,
    pub keys: Vec<String>,
}

/// Precomputed metadata for a single host, ready to serve.
#[derive(Debug, Clone)]
pub struct HostMetadata {
    pub instance_id: String,
    pub hostname: String,
    pub local_hostname: String,
    pub local_ipv4: String,
    pub availability_zone: String,
    pub public_keys: Vec<PublicKeyEntry>,
    pub user_data: String,
    /// Structured cloud-config for ignition/kickstart generation.
    pub cloud_config: CloudConfig,
}

/// User account entry for cloud-config user-data.
#[derive(Debug, Clone, Serialize)]
pub struct CloudConfigUser {
    pub name: String,
    pub uid: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<String>,
    pub shell: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sudo: Option<String>,
    pub ssh_authorized_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CloudConfig {
    pub users: Vec<CloudConfigUser>,
}
