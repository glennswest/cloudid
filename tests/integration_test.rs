use cloudid::cache::{AppState, BmhState, ContainerInfo, ContainerState, IdentityState};
use cloudid::config::{
    AmoConfig, Config, MetadataConfig, MkubeConfig, ServerConfig, TemplatesConfig,
};
use cloudid::model::*;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

/// Build a minimal Config suitable for tests (no file I/O).
fn test_config() -> Config {
    Config {
        server: ServerConfig {
            metadata_addr: "127.0.0.1:0".to_string(),
        },
        amo: AmoConfig {
            nats_url: "nats://127.0.0.1:4222".to_string(),
        },
        mkube: MkubeConfig {
            url: "http://127.0.0.1:8082".to_string(),
        },
        metadata: MetadataConfig {
            domain_suffix: ".test.lo".to_string(),
            availability_zone: "test-az".to_string(),
            cache_rebuild_interval_secs: 30,
            dhcp_sources: vec![],
        },
        static_users: vec![],
        static_host_access: vec![],
        templates: TemplatesConfig::default(),
    }
}

/// Create a test user resource.
fn make_user(name: &str, uid: u32, key: &str) -> UserResource {
    Resource {
        kind: "User".to_string(),
        metadata: ResourceMeta {
            name: name.to_string(),
            namespace: String::new(),
            labels: HashMap::new(),
            annotations: HashMap::new(),
        },
        spec: UserSpec {
            display_name: name.to_string(),
            email: None,
            org: String::new(),
            uid,
            gid: uid,
            shell: "/bin/bash".to_string(),
            ssh_public_keys: vec![SshPublicKey {
                name: "test-key".to_string(),
                key: key.to_string(),
            }],
            groups: vec!["wheel".to_string()],
        },
        status: Some(ResourceStatus { enabled: true }),
    }
}

/// Create a host access rule.
fn make_host_access(
    name: &str,
    users: Vec<&str>,
    hosts: Vec<&str>,
    ssh_users: Vec<&str>,
    sudo: bool,
) -> HostAccessResource {
    Resource {
        kind: "HostAccess".to_string(),
        metadata: ResourceMeta {
            name: name.to_string(),
            namespace: String::new(),
            labels: HashMap::new(),
            annotations: HashMap::new(),
        },
        spec: HostAccessSpec {
            subjects: users
                .into_iter()
                .map(|u| Subject {
                    kind: SubjectKind::User,
                    name: u.to_string(),
                })
                .collect(),
            targets: HostAccessTargets {
                hosts: hosts.into_iter().map(|h| h.to_string()).collect(),
                host_groups: vec![],
                host_selectors: vec![],
            },
            ssh_users: ssh_users.into_iter().map(|s| s.to_string()).collect(),
            sudo,
        },
        status: None,
    }
}

/// Create a host access rule with group subjects.
fn make_group_access(
    name: &str,
    groups: Vec<&str>,
    hosts: Vec<&str>,
    ssh_users: Vec<&str>,
    sudo: bool,
) -> HostAccessResource {
    Resource {
        kind: "HostAccess".to_string(),
        metadata: ResourceMeta {
            name: name.to_string(),
            namespace: String::new(),
            labels: HashMap::new(),
            annotations: HashMap::new(),
        },
        spec: HostAccessSpec {
            subjects: groups
                .into_iter()
                .map(|g| Subject {
                    kind: SubjectKind::Group,
                    name: g.to_string(),
                })
                .collect(),
            targets: HostAccessTargets {
                hosts: hosts.into_iter().map(|h| h.to_string()).collect(),
                host_groups: vec![],
                host_selectors: vec![],
            },
            ssh_users: ssh_users.into_iter().map(|s| s.to_string()).collect(),
            sudo,
        },
        status: None,
    }
}

/// Create a host access rule with label selectors.
fn make_label_selector_access(
    name: &str,
    users: Vec<&str>,
    selectors: Vec<HashMap<String, String>>,
    ssh_users: Vec<&str>,
) -> HostAccessResource {
    Resource {
        kind: "HostAccess".to_string(),
        metadata: ResourceMeta {
            name: name.to_string(),
            namespace: String::new(),
            labels: HashMap::new(),
            annotations: HashMap::new(),
        },
        spec: HostAccessSpec {
            subjects: users
                .into_iter()
                .map(|u| Subject {
                    kind: SubjectKind::User,
                    name: u.to_string(),
                })
                .collect(),
            targets: HostAccessTargets {
                hosts: vec![],
                host_groups: vec![],
                host_selectors: selectors,
            },
            ssh_users: ssh_users.into_iter().map(|s| s.to_string()).collect(),
            sudo: false,
        },
        status: None,
    }
}

/// Create a host access rule with host_groups.
fn make_hostgroup_access(
    name: &str,
    users: Vec<&str>,
    host_groups: Vec<&str>,
    ssh_users: Vec<&str>,
) -> HostAccessResource {
    Resource {
        kind: "HostAccess".to_string(),
        metadata: ResourceMeta {
            name: name.to_string(),
            namespace: String::new(),
            labels: HashMap::new(),
            annotations: HashMap::new(),
        },
        spec: HostAccessSpec {
            subjects: users
                .into_iter()
                .map(|u| Subject {
                    kind: SubjectKind::User,
                    name: u.to_string(),
                })
                .collect(),
            targets: HostAccessTargets {
                hosts: vec![],
                host_groups: host_groups.into_iter().map(|h| h.to_string()).collect(),
                host_selectors: vec![],
            },
            ssh_users: ssh_users.into_iter().map(|s| s.to_string()).collect(),
            sudo: false,
        },
        status: None,
    }
}

/// Create a group resource.
fn make_group(name: &str, members: Vec<&str>) -> GroupResource {
    Resource {
        kind: "Group".to_string(),
        metadata: ResourceMeta {
            name: name.to_string(),
            namespace: String::new(),
            labels: HashMap::new(),
            annotations: HashMap::new(),
        },
        spec: GroupSpec {
            display_name: name.to_string(),
            gid: 0,
            members: members.into_iter().map(|m| m.to_string()).collect(),
            org: String::new(),
        },
        status: None,
    }
}

/// Create a host group resource.
fn make_host_group(name: &str, hosts: Vec<&str>) -> HostGroupResource {
    Resource {
        kind: "HostGroup".to_string(),
        metadata: ResourceMeta {
            name: name.to_string(),
            namespace: String::new(),
            labels: HashMap::new(),
            annotations: HashMap::new(),
        },
        spec: HostGroupSpec {
            hosts: hosts.into_iter().map(|h| h.to_string()).collect(),
            labels: HashMap::new(),
        },
        status: None,
    }
}

/// Build AppState with injected identity + BMH data, rebuild cache, return Arc<AppState>.
async fn build_state(
    identity: IdentityState,
    bmh: BmhState,
    containers: ContainerState,
) -> Arc<AppState> {
    let config = test_config();
    let state = AppState::new(config).await;

    {
        let mut id = state.identity.write().await;
        *id = identity;
    }
    {
        let mut b = state.bmh.write().await;
        *b = bmh;
    }
    {
        let mut c = state.containers.write().await;
        *c = containers;
    }

    state.rebuild_cache().await;
    state
}

// ---- Tests ----

#[tokio::test]
async fn test_cache_rebuild_serves_known_host() {
    let mut identity = IdentityState::default();
    identity
        .users
        .insert("alice".into(), make_user("alice", 1000, "ssh-ed25519 AAAA alice@test"));
    identity.host_access.insert(
        "rule-1".into(),
        make_host_access("rule-1", vec!["alice"], vec!["server1"], vec!["root", "core"], true),
    );

    let mut bmh = BmhState::default();
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    bmh.ip_to_hostname.insert(ip, "server1".to_string());

    let state = build_state(identity, bmh, ContainerState::default()).await;

    let meta = state.get_metadata(&ip);
    assert!(meta.is_some(), "known host should be in cache");

    let meta = meta.unwrap();
    assert_eq!(meta.instance_id, "server1");
    assert_eq!(meta.hostname, "server1.test.lo");
    assert_eq!(meta.local_ipv4, "10.0.0.1");
    assert_eq!(meta.availability_zone, "test-az");
    assert_eq!(meta.public_keys.len(), 2);

    // Keys sorted alphabetically by ssh_user: core, root
    assert_eq!(meta.public_keys[0].ssh_user, "core");
    assert_eq!(meta.public_keys[1].ssh_user, "root");
    assert_eq!(meta.public_keys[0].keys, vec!["ssh-ed25519 AAAA alice@test"]);
    assert_eq!(meta.public_keys[1].keys, vec!["ssh-ed25519 AAAA alice@test"]);

    // Cloud config has alice with sudo
    assert_eq!(meta.cloud_config.users.len(), 1);
    assert_eq!(meta.cloud_config.users[0].name, "alice");
    assert!(meta.cloud_config.users[0].sudo.is_some());
}

#[tokio::test]
async fn test_unknown_ip_returns_none() {
    let mut identity = IdentityState::default();
    identity
        .users
        .insert("alice".into(), make_user("alice", 1000, "ssh-ed25519 AAAA alice@test"));
    identity.host_access.insert(
        "rule-1".into(),
        make_host_access("rule-1", vec!["alice"], vec!["server1"], vec!["root"], false),
    );

    let mut bmh = BmhState::default();
    bmh.ip_to_hostname
        .insert("10.0.0.1".parse().unwrap(), "server1".to_string());

    let state = build_state(identity, bmh, ContainerState::default()).await;

    let unknown_ip: IpAddr = "10.0.0.99".parse().unwrap();
    assert!(state.get_metadata(&unknown_ip).is_none());
    assert!(state.is_unknown_ip(&unknown_ip));
}

#[tokio::test]
async fn test_cache_rebuild_on_identity_change() {
    let mut identity = IdentityState::default();
    identity
        .users
        .insert("alice".into(), make_user("alice", 1000, "ssh-ed25519 AAAA alice@test"));
    identity.host_access.insert(
        "rule-1".into(),
        make_host_access("rule-1", vec!["alice"], vec!["server1"], vec!["root"], false),
    );

    let mut bmh = BmhState::default();
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    bmh.ip_to_hostname.insert(ip, "server1".to_string());

    let state = build_state(identity, bmh, ContainerState::default()).await;

    // Verify initial state
    let meta = state.get_metadata(&ip).unwrap();
    assert_eq!(meta.public_keys[0].keys.len(), 1);

    // Add a second user with access to the same host
    {
        let mut id = state.identity.write().await;
        id.users
            .insert("bob".into(), make_user("bob", 1001, "ssh-rsa BBBB bob@test"));
        id.host_access.insert(
            "rule-2".into(),
            make_host_access("rule-2", vec!["bob"], vec!["server1"], vec!["root"], false),
        );
    }

    // Rebuild cache
    state.rebuild_cache().await;

    // Verify both users' keys are now present for root
    let meta = state.get_metadata(&ip).unwrap();
    let root_keys = meta
        .public_keys
        .iter()
        .find(|pk| pk.ssh_user == "root")
        .unwrap();
    assert_eq!(root_keys.keys.len(), 2);
    assert!(root_keys.keys.contains(&"ssh-ed25519 AAAA alice@test".to_string()));
    assert!(root_keys.keys.contains(&"ssh-rsa BBBB bob@test".to_string()));

    // Cloud config has both users
    assert_eq!(meta.cloud_config.users.len(), 2);
}

#[tokio::test]
async fn test_container_identity_via_namespace_owner() {
    let mut identity = IdentityState::default();
    identity
        .users
        .insert("gwest".into(), make_user("gwest", 1000, "ssh-ed25519 CCCC gwest@mac"));

    let mut containers = ContainerState::default();
    let ip: IpAddr = "10.1.0.50".parse().unwrap();
    containers.ip_to_container.insert(
        ip,
        ContainerInfo {
            namespace: "prod".to_string(),
            pod_name: "myapp".to_string(),
            container_name: "main".to_string(),
            hostname: "main.myapp".to_string(),
        },
    );
    containers
        .namespace_owners
        .insert("prod".to_string(), "gwest".to_string());

    let state = build_state(identity, BmhState::default(), containers).await;

    let meta = state.get_metadata(&ip);
    assert!(meta.is_some(), "container should be in cache");

    let meta = meta.unwrap();
    assert_eq!(meta.instance_id, "prod/myapp");
    assert_eq!(meta.hostname, "main.myapp.test.lo");
    assert_eq!(meta.local_ipv4, "10.1.0.50");

    // Container gets: owner, admin, root
    assert_eq!(meta.public_keys.len(), 3);
    assert_eq!(meta.public_keys[0].ssh_user, "gwest");
    assert_eq!(meta.public_keys[1].ssh_user, "admin");
    assert_eq!(meta.public_keys[2].ssh_user, "root");

    // Cloud config: owner + root
    assert_eq!(meta.cloud_config.users.len(), 2);
    assert_eq!(meta.cloud_config.users[0].name, "gwest");
    assert_eq!(meta.cloud_config.users[1].name, "root");
    assert!(meta.cloud_config.users[0].sudo.is_some());
}

#[tokio::test]
async fn test_disabled_user_excluded_from_cache() {
    let mut identity = IdentityState::default();
    let mut user = make_user("alice", 1000, "ssh-ed25519 AAAA alice@test");
    user.status = Some(ResourceStatus { enabled: false });
    identity.users.insert("alice".into(), user);
    identity.host_access.insert(
        "rule-1".into(),
        make_host_access("rule-1", vec!["alice"], vec!["server1"], vec!["root"], false),
    );

    let mut bmh = BmhState::default();
    bmh.ip_to_hostname
        .insert("10.0.0.1".parse().unwrap(), "server1".to_string());

    let state = build_state(identity, bmh, ContainerState::default()).await;

    // Disabled user means no keys resolved, so no cache entry
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    assert!(state.get_metadata(&ip).is_none());
}

#[tokio::test]
async fn test_group_expansion_in_host_access() {
    let mut identity = IdentityState::default();
    identity
        .users
        .insert("alice".into(), make_user("alice", 1000, "ssh-ed25519 AAAA alice@test"));
    identity
        .users
        .insert("bob".into(), make_user("bob", 1001, "ssh-rsa BBBB bob@test"));
    identity
        .groups
        .insert("engineering".into(), make_group("engineering", vec!["alice", "bob"]));
    identity.host_access.insert(
        "eng-access".into(),
        make_group_access("eng-access", vec!["engineering"], vec!["server1"], vec!["core"], true),
    );

    let mut bmh = BmhState::default();
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    bmh.ip_to_hostname.insert(ip, "server1".to_string());

    let state = build_state(identity, bmh, ContainerState::default()).await;

    let meta = state.get_metadata(&ip).unwrap();
    let core_keys = meta
        .public_keys
        .iter()
        .find(|pk| pk.ssh_user == "core")
        .unwrap();
    assert_eq!(core_keys.keys.len(), 2);

    // Both group members in cloud config
    assert_eq!(meta.cloud_config.users.len(), 2);
    let names: Vec<&str> = meta.cloud_config.users.iter().map(|u| u.name.as_str()).collect();
    assert!(names.contains(&"alice"));
    assert!(names.contains(&"bob"));
}

#[tokio::test]
async fn test_label_selector_matching() {
    let mut identity = IdentityState::default();
    identity
        .users
        .insert("alice".into(), make_user("alice", 1000, "ssh-ed25519 AAAA alice@test"));

    let mut selector = HashMap::new();
    selector.insert("tier".to_string(), "web".to_string());
    identity.host_access.insert(
        "web-access".into(),
        make_label_selector_access("web-access", vec!["alice"], vec![selector], vec!["root"]),
    );

    let mut bmh = BmhState::default();
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    bmh.ip_to_hostname.insert(ip, "web-server".to_string());

    // web-server has matching labels
    let mut labels = HashMap::new();
    labels.insert("tier".to_string(), "web".to_string());
    bmh.host_labels.insert("web-server".to_string(), labels);

    let state = build_state(identity, bmh, ContainerState::default()).await;

    let meta = state.get_metadata(&ip);
    assert!(meta.is_some(), "label selector should match");
    assert_eq!(meta.unwrap().instance_id, "web-server");
}

#[tokio::test]
async fn test_label_selector_no_match() {
    let mut identity = IdentityState::default();
    identity
        .users
        .insert("alice".into(), make_user("alice", 1000, "ssh-ed25519 AAAA alice@test"));

    let mut selector = HashMap::new();
    selector.insert("tier".to_string(), "web".to_string());
    identity.host_access.insert(
        "web-access".into(),
        make_label_selector_access("web-access", vec!["alice"], vec![selector], vec!["root"]),
    );

    let mut bmh = BmhState::default();
    let ip: IpAddr = "10.0.0.2".parse().unwrap();
    bmh.ip_to_hostname.insert(ip, "db-server".to_string());

    // db-server has different labels
    let mut labels = HashMap::new();
    labels.insert("tier".to_string(), "database".to_string());
    bmh.host_labels.insert("db-server".to_string(), labels);

    let state = build_state(identity, bmh, ContainerState::default()).await;

    assert!(state.get_metadata(&ip).is_none(), "label selector should not match");
}

#[tokio::test]
async fn test_host_group_matching() {
    let mut identity = IdentityState::default();
    identity
        .users
        .insert("alice".into(), make_user("alice", 1000, "ssh-ed25519 AAAA alice@test"));
    identity
        .host_groups
        .insert("web-tier".into(), make_host_group("web-tier", vec!["web1", "web2", "web3"]));
    identity.host_access.insert(
        "web-access".into(),
        make_hostgroup_access("web-access", vec!["alice"], vec!["web-tier"], vec!["root"]),
    );

    let mut bmh = BmhState::default();
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    bmh.ip_to_hostname.insert(ip, "web2".to_string());

    let state = build_state(identity, bmh, ContainerState::default()).await;

    let meta = state.get_metadata(&ip);
    assert!(meta.is_some(), "host group member should match");
    assert_eq!(meta.unwrap().instance_id, "web2");
}

#[tokio::test]
async fn test_host_not_in_group_excluded() {
    let mut identity = IdentityState::default();
    identity
        .users
        .insert("alice".into(), make_user("alice", 1000, "ssh-ed25519 AAAA alice@test"));
    identity
        .host_groups
        .insert("web-tier".into(), make_host_group("web-tier", vec!["web1", "web2"]));
    identity.host_access.insert(
        "web-access".into(),
        make_hostgroup_access("web-access", vec!["alice"], vec!["web-tier"], vec!["root"]),
    );

    let mut bmh = BmhState::default();
    let ip: IpAddr = "10.0.0.5".parse().unwrap();
    bmh.ip_to_hostname.insert(ip, "db1".to_string());

    let state = build_state(identity, bmh, ContainerState::default()).await;

    assert!(state.get_metadata(&ip).is_none(), "host not in group should not match");
}

#[tokio::test]
async fn test_resolve_on_demand_fallback() {
    let mut identity = IdentityState::default();
    identity
        .users
        .insert("alice".into(), make_user("alice", 1000, "ssh-ed25519 AAAA alice@test"));
    identity.host_access.insert(
        "rule-1".into(),
        make_host_access("rule-1", vec!["alice"], vec!["server1"], vec!["root"], false),
    );

    let mut bmh = BmhState::default();
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    bmh.ip_to_hostname.insert(ip, "server1".to_string());

    let state = build_state(identity, bmh, ContainerState::default()).await;

    // Clear the cache to simulate a miss
    state.metadata_cache.clear();
    assert!(state.get_metadata(&ip).is_none());

    // resolve_on_demand should still work by reading identity + bmh directly
    let meta = state.resolve_on_demand(&ip).await;
    assert!(meta.is_some());
    assert_eq!(meta.unwrap().instance_id, "server1");
}

#[tokio::test]
async fn test_multiple_rules_merge_keys() {
    let mut identity = IdentityState::default();
    identity
        .users
        .insert("alice".into(), make_user("alice", 1000, "ssh-ed25519 AAAA alice@test"));
    identity
        .users
        .insert("bob".into(), make_user("bob", 1001, "ssh-rsa BBBB bob@test"));

    // Two rules granting different users access to the same host
    identity.host_access.insert(
        "rule-alice".into(),
        make_host_access("rule-alice", vec!["alice"], vec!["server1"], vec!["root"], true),
    );
    identity.host_access.insert(
        "rule-bob".into(),
        make_host_access("rule-bob", vec!["bob"], vec!["server1"], vec!["root"], false),
    );

    let mut bmh = BmhState::default();
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    bmh.ip_to_hostname.insert(ip, "server1".to_string());

    let state = build_state(identity, bmh, ContainerState::default()).await;

    let meta = state.get_metadata(&ip).unwrap();
    let root_keys = meta
        .public_keys
        .iter()
        .find(|pk| pk.ssh_user == "root")
        .unwrap();
    assert_eq!(root_keys.keys.len(), 2, "both users' keys should be merged for root");
}

#[tokio::test]
async fn test_wildcard_matches_any_host() {
    let mut identity = IdentityState::default();
    identity
        .users
        .insert("admin".into(), make_user("admin", 0, "ssh-ed25519 ADMIN admin@infra"));
    identity.host_access.insert(
        "admin-all".into(),
        make_host_access("admin-all", vec!["admin"], vec!["*"], vec!["root"], true),
    );

    let mut bmh = BmhState::default();
    let ip1: IpAddr = "10.0.0.1".parse().unwrap();
    let ip2: IpAddr = "10.0.0.2".parse().unwrap();
    bmh.ip_to_hostname.insert(ip1, "server1".to_string());
    bmh.ip_to_hostname.insert(ip2, "server2".to_string());

    let state = build_state(identity, bmh, ContainerState::default()).await;

    assert!(state.get_metadata(&ip1).is_some());
    assert!(state.get_metadata(&ip2).is_some());
    assert_eq!(state.get_metadata(&ip1).unwrap().instance_id, "server1");
    assert_eq!(state.get_metadata(&ip2).unwrap().instance_id, "server2");
}

#[tokio::test]
async fn test_container_disabled_owner_excluded() {
    let mut identity = IdentityState::default();
    let mut user = make_user("gwest", 1000, "ssh-ed25519 CCCC gwest@mac");
    user.status = Some(ResourceStatus { enabled: false });
    identity.users.insert("gwest".into(), user);

    let mut containers = ContainerState::default();
    let ip: IpAddr = "10.1.0.50".parse().unwrap();
    containers.ip_to_container.insert(
        ip,
        ContainerInfo {
            namespace: "prod".to_string(),
            pod_name: "myapp".to_string(),
            container_name: "main".to_string(),
            hostname: "main.myapp".to_string(),
        },
    );
    containers
        .namespace_owners
        .insert("prod".to_string(), "gwest".to_string());

    let state = build_state(identity, BmhState::default(), containers).await;

    assert!(state.get_metadata(&ip).is_none(), "disabled owner should not produce metadata");
}

#[tokio::test]
async fn test_container_missing_owner_excluded() {
    let identity = IdentityState::default(); // No users at all

    let mut containers = ContainerState::default();
    let ip: IpAddr = "10.1.0.50".parse().unwrap();
    containers.ip_to_container.insert(
        ip,
        ContainerInfo {
            namespace: "prod".to_string(),
            pod_name: "myapp".to_string(),
            container_name: "main".to_string(),
            hostname: "main.myapp".to_string(),
        },
    );
    containers
        .namespace_owners
        .insert("prod".to_string(), "nobody".to_string());

    let state = build_state(identity, BmhState::default(), containers).await;

    assert!(state.get_metadata(&ip).is_none(), "missing owner should not produce metadata");
}

#[tokio::test]
async fn test_container_no_namespace_owner() {
    let mut identity = IdentityState::default();
    identity
        .users
        .insert("gwest".into(), make_user("gwest", 1000, "ssh-ed25519 CCCC gwest@mac"));

    let mut containers = ContainerState::default();
    let ip: IpAddr = "10.1.0.50".parse().unwrap();
    containers.ip_to_container.insert(
        ip,
        ContainerInfo {
            namespace: "orphan".to_string(),
            pod_name: "myapp".to_string(),
            container_name: "main".to_string(),
            hostname: "main.myapp".to_string(),
        },
    );
    // No namespace_owners entry for "orphan"

    let state = build_state(identity, BmhState::default(), containers).await;

    assert!(
        state.get_metadata(&ip).is_none(),
        "container without namespace owner should not produce metadata"
    );
}

#[tokio::test]
async fn test_imds_token_lifecycle() {
    let state = build_state(
        IdentityState::default(),
        BmhState::default(),
        ContainerState::default(),
    )
    .await;

    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    let token = state.generate_imds_token(ip, 300);

    // Token should be valid
    assert!(token.starts_with("cloudid-"));
    let validated_ip = state.validate_imds_token(&token);
    assert_eq!(validated_ip, Some(ip));

    // Invalid token should fail
    assert!(state.validate_imds_token("bogus-token").is_none());
}

#[tokio::test]
async fn test_user_data_cloud_config_format() {
    let mut identity = IdentityState::default();
    identity
        .users
        .insert("alice".into(), make_user("alice", 1000, "ssh-ed25519 AAAA alice@test"));
    identity.host_access.insert(
        "rule-1".into(),
        make_host_access("rule-1", vec!["alice"], vec!["server1"], vec!["root"], true),
    );

    let mut bmh = BmhState::default();
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    bmh.ip_to_hostname.insert(ip, "server1".to_string());

    let state = build_state(identity, bmh, ContainerState::default()).await;

    let meta = state.get_metadata(&ip).unwrap();
    assert!(meta.user_data.starts_with("#cloud-config\n"));
    assert!(meta.user_data.contains("alice"));
    assert!(meta.user_data.contains("ssh-ed25519 AAAA alice@test"));
}

#[tokio::test]
async fn test_mixed_bmh_and_containers() {
    let mut identity = IdentityState::default();
    identity
        .users
        .insert("alice".into(), make_user("alice", 1000, "ssh-ed25519 AAAA alice@test"));
    identity.host_access.insert(
        "rule-1".into(),
        make_host_access("rule-1", vec!["alice"], vec!["server1"], vec!["root"], false),
    );

    let mut bmh = BmhState::default();
    let bmh_ip: IpAddr = "10.0.0.1".parse().unwrap();
    bmh.ip_to_hostname.insert(bmh_ip, "server1".to_string());

    let mut containers = ContainerState::default();
    let container_ip: IpAddr = "10.1.0.50".parse().unwrap();
    containers.ip_to_container.insert(
        container_ip,
        ContainerInfo {
            namespace: "prod".to_string(),
            pod_name: "myapp".to_string(),
            container_name: "main".to_string(),
            hostname: "main.myapp".to_string(),
        },
    );
    containers
        .namespace_owners
        .insert("prod".to_string(), "alice".to_string());

    let state = build_state(identity, bmh, containers).await;

    // Both should be in cache
    assert!(state.get_metadata(&bmh_ip).is_some());
    assert!(state.get_metadata(&container_ip).is_some());

    // BMH entry
    let bmh_meta = state.get_metadata(&bmh_ip).unwrap();
    assert_eq!(bmh_meta.instance_id, "server1");

    // Container entry
    let ctr_meta = state.get_metadata(&container_ip).unwrap();
    assert_eq!(ctr_meta.instance_id, "prod/myapp");
}
