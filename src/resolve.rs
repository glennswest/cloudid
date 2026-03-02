use crate::cache::IdentityState;
use crate::config::MetadataConfig;
use crate::model::{
    CloudConfig, CloudConfigUser, HostAccessResource, HostMetadata, PublicKeyEntry, Subject,
    SubjectKind,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::IpAddr;

/// Resolve metadata for a host given its IP and hostname.
///
/// Pipeline:
///   1. hostname -> matching HostAccess rules
///   2. Expand subjects (users + groups) from matching rules
///   3. Collect SSH keys per ssh_user
///   4. Build HostMetadata with cloud-config
pub fn resolve_host(
    ip: IpAddr,
    hostname: &str,
    host_labels: Option<&HashMap<String, String>>,
    identity: &IdentityState,
    config: &MetadataConfig,
) -> Option<HostMetadata> {
    // Step 1: Find all HostAccess rules matching this hostname
    let matching_rules = find_matching_rules(hostname, host_labels, identity);

    if matching_rules.is_empty() {
        return None;
    }

    // Step 2+3: Collect users and SSH keys per ssh_user
    let mut keys_by_ssh_user: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut cloud_config_users: BTreeMap<String, CloudConfigUser> = BTreeMap::new();
    let mut seen_users: HashSet<String> = HashSet::new();

    for rule in &matching_rules {
        let sudo_str = if rule.spec.sudo {
            Some("ALL=(ALL) NOPASSWD:ALL".to_string())
        } else {
            None
        };

        let expanded = expand_subjects(&rule.spec.subjects, identity);

        for username in expanded {
            let user_res = match identity.users.get(&username) {
                Some(u) => u,
                None => continue,
            };

            // Skip disabled users
            if let Some(ref status) = user_res.status {
                if !status.enabled {
                    continue;
                }
            }

            let user_keys: Vec<String> = user_res
                .spec
                .ssh_public_keys
                .iter()
                .map(|k| k.key.clone())
                .collect();

            // Map keys to each ssh_user this rule grants
            for ssh_user in &rule.spec.ssh_users {
                keys_by_ssh_user
                    .entry(ssh_user.clone())
                    .or_default()
                    .extend(user_keys.clone());
            }

            // Build cloud-config user entry (deduplicate)
            if seen_users.insert(username.clone()) {
                let user_groups: Vec<String> = user_res.spec.groups.clone();

                cloud_config_users.insert(
                    username.clone(),
                    CloudConfigUser {
                        name: username.clone(),
                        uid: user_res.spec.uid.to_string(),
                        groups: user_groups,
                        shell: user_res.spec.shell.clone(),
                        sudo: sudo_str.clone(),
                        ssh_authorized_keys: user_keys,
                    },
                );
            }
        }
    }

    // Deduplicate keys per ssh_user
    for keys in keys_by_ssh_user.values_mut() {
        keys.sort();
        keys.dedup();
    }

    let public_keys: Vec<PublicKeyEntry> = keys_by_ssh_user
        .into_iter()
        .map(|(ssh_user, keys)| PublicKeyEntry { ssh_user, keys })
        .collect();

    // Build cloud-config YAML
    let cloud_config = CloudConfig {
        users: cloud_config_users.into_values().collect(),
    };
    let user_data = format!(
        "#cloud-config\n{}",
        serde_json::to_string_pretty(&cloud_config).unwrap_or_default()
    );

    let fqdn = format!("{}{}", hostname, config.domain_suffix);

    Some(HostMetadata {
        instance_id: hostname.to_string(),
        hostname: fqdn,
        local_hostname: hostname.to_string(),
        local_ipv4: ip.to_string(),
        availability_zone: config.availability_zone.clone(),
        public_keys,
        user_data,
    })
}

/// Find all HostAccess rules that match a given hostname.
fn find_matching_rules<'a>(
    hostname: &str,
    host_labels: Option<&HashMap<String, String>>,
    identity: &'a IdentityState,
) -> Vec<&'a HostAccessResource> {
    identity
        .host_access
        .values()
        .filter(|rule| {
            let targets = &rule.spec.targets;

            // Direct hostname match
            if targets.hosts.iter().any(|h| h == hostname) {
                return true;
            }

            // HostGroup match
            if targets.host_groups.iter().any(|hg_name| {
                identity
                    .host_groups
                    .get(hg_name)
                    .map(|hg| hg.spec.hosts.iter().any(|h| h == hostname))
                    .unwrap_or(false)
            }) {
                return true;
            }

            // Label selector match
            if let Some(labels) = host_labels {
                if targets.host_selectors.iter().any(|selector| {
                    selector
                        .iter()
                        .all(|(k, v)| labels.get(k).map(|lv| lv == v).unwrap_or(false))
                }) {
                    return true;
                }
            }

            false
        })
        .collect()
}

/// Expand subjects (users and groups) into a list of usernames.
fn expand_subjects(subjects: &[Subject], identity: &IdentityState) -> Vec<String> {
    let mut usernames = Vec::new();
    let mut seen = HashSet::new();

    for subject in subjects {
        match subject.kind {
            SubjectKind::User => {
                if seen.insert(subject.name.clone()) {
                    usernames.push(subject.name.clone());
                }
            }
            SubjectKind::Group => {
                if let Some(group) = identity.groups.get(&subject.name) {
                    for member in &group.spec.members {
                        if seen.insert(member.clone()) {
                            usernames.push(member.clone());
                        }
                    }
                }
            }
        }
    }

    usernames
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn make_identity() -> IdentityState {
        let mut state = IdentityState::default();

        state.users.insert(
            "gwest".to_string(),
            Resource {
                kind: "User".to_string(),
                metadata: ResourceMeta {
                    name: "gwest".to_string(),
                    namespace: String::new(),
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                },
                spec: UserSpec {
                    display_name: "Glenn West".to_string(),
                    email: Some("gwest@acme.lo".to_string()),
                    org: "acme".to_string(),
                    uid: 1000,
                    gid: 1000,
                    shell: "/bin/bash".to_string(),
                    ssh_public_keys: vec![SshPublicKey {
                        name: "macbook".to_string(),
                        key: "ssh-rsa AAAA_TEST_KEY gwest@macbook".to_string(),
                    }],
                    groups: vec!["wheel".to_string(), "engineering".to_string()],
                },
                status: Some(ResourceStatus { enabled: true }),
            },
        );

        state.host_access.insert(
            "eng-servers".to_string(),
            Resource {
                kind: "HostAccess".to_string(),
                metadata: ResourceMeta {
                    name: "eng-servers".to_string(),
                    namespace: String::new(),
                    labels: HashMap::new(),
                    annotations: HashMap::new(),
                },
                spec: HostAccessSpec {
                    subjects: vec![Subject {
                        kind: SubjectKind::User,
                        name: "gwest".to_string(),
                    }],
                    targets: HostAccessTargets {
                        hosts: vec!["server1".to_string()],
                        host_groups: vec![],
                        host_selectors: vec![],
                    },
                    ssh_users: vec!["root".to_string(), "core".to_string()],
                    sudo: true,
                },
                status: None,
            },
        );

        state
    }

    #[test]
    fn test_resolve_known_host() {
        let identity = make_identity();
        let config = MetadataConfig {
            domain_suffix: ".g10.lo".to_string(),
            availability_zone: "gt".to_string(),
            cache_rebuild_interval_secs: 30,
            dhcp_sources: vec![],
        };

        let ip: IpAddr = "192.168.10.10".parse().unwrap();
        let result = resolve_host(ip, "server1", None, &identity, &config);
        assert!(result.is_some());

        let meta = result.unwrap();
        assert_eq!(meta.instance_id, "server1");
        assert_eq!(meta.hostname, "server1.g10.lo");
        assert_eq!(meta.local_ipv4, "192.168.10.10");
        assert_eq!(meta.public_keys.len(), 2); // root + core
        assert_eq!(meta.public_keys[0].ssh_user, "core");
        assert_eq!(meta.public_keys[1].ssh_user, "root");
        assert!(meta.public_keys[0].keys[0].contains("AAAA_TEST_KEY"));
    }

    #[test]
    fn test_resolve_unknown_host() {
        let identity = make_identity();
        let config = MetadataConfig {
            domain_suffix: ".g10.lo".to_string(),
            availability_zone: "gt".to_string(),
            cache_rebuild_interval_secs: 30,
            dhcp_sources: vec![],
        };

        let ip: IpAddr = "192.168.10.99".parse().unwrap();
        let result = resolve_host(ip, "unknown-host", None, &identity, &config);
        assert!(result.is_none());
    }

    #[test]
    fn test_disabled_user_excluded() {
        let mut identity = make_identity();
        if let Some(user) = identity.users.get_mut("gwest") {
            user.status = Some(ResourceStatus { enabled: false });
        }

        let config = MetadataConfig {
            domain_suffix: ".g10.lo".to_string(),
            availability_zone: "gt".to_string(),
            cache_rebuild_interval_secs: 30,
            dhcp_sources: vec![],
        };

        let ip: IpAddr = "192.168.10.10".parse().unwrap();
        let result = resolve_host(ip, "server1", None, &identity, &config);
        // No enabled users -> no metadata
        assert!(result.is_none());
    }
}
