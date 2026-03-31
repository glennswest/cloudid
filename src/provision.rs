use crate::cache::AppState;
use crate::model::{BareMetalHost, HostMetadata};
use crate::templates::{self, TemplateFormat};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use tracing::{debug, info};

// --- Ignition v3.4.0 types for serialization ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnitionConfig {
    pub ignition: IgnitionMeta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passwd: Option<IgnitionPasswd>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<IgnitionStorage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub systemd: Option<IgnitionSystemd>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnitionMeta {
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnitionPasswd {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub users: Vec<IgnitionUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IgnitionUser {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_authorized_keys: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnitionStorage {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<IgnitionFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnitionFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overwrite: Option<bool>,
    pub contents: IgnitionFileContents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnitionFileContents {
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnitionSystemd {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub units: Vec<IgnitionUnit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnitionUnit {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contents: Option<String>,
}

/// Build the ignition config for a host.
///
/// If the BMH has a base ignition config in `spec.ignition`, merge SSH keys
/// from the identity pipeline into it. Otherwise generate a default config.
pub fn build_ignition(meta: &HostMetadata, bmh: Option<&BareMetalHost>) -> String {
    let base = bmh.and_then(|b| b.spec.ignition.as_ref());

    match base {
        Some(base_json) => merge_ignition(base_json, meta),
        None => generate_ignition(meta),
    }
}

/// Build the kickstart config for a host.
///
/// If the BMH has a base kickstart config in `spec.kickstart`, merge SSH keys
/// from the identity pipeline into it. Otherwise generate a default config.
pub fn build_kickstart(meta: &HostMetadata, bmh: Option<&BareMetalHost>) -> String {
    let base = bmh.and_then(|b| b.spec.kickstart.as_ref());

    match base {
        Some(base_text) => merge_kickstart(base_text, meta),
        None => generate_kickstart(meta),
    }
}

/// Merge SSH keys and users from the identity pipeline into a base Ignition config.
fn merge_ignition(base: &Value, meta: &HostMetadata) -> String {
    let mut config: Value = base.clone();

    // Ensure ignition.version exists
    if config.get("ignition").is_none() {
        config["ignition"] = serde_json::json!({"version": "3.4.0"});
    }

    // Build user entries from identity pipeline
    let identity_users = build_ignition_users(meta);

    // Get or create passwd.users array
    let passwd = config
        .as_object_mut()
        .unwrap()
        .entry("passwd")
        .or_insert_with(|| serde_json::json!({}));
    let users_array = passwd
        .as_object_mut()
        .unwrap()
        .entry("users")
        .or_insert_with(|| serde_json::json!([]));

    let existing_users: HashSet<String> = users_array
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|u| u.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    // Merge: for existing users, append SSH keys; for new users, add full entry
    if let Some(arr) = users_array.as_array_mut() {
        for ign_user in &identity_users {
            if existing_users.contains(&ign_user.name) {
                // Find and merge SSH keys into existing user
                for existing in arr.iter_mut() {
                    if existing.get("name").and_then(|n| n.as_str()) == Some(&ign_user.name) {
                        merge_ssh_keys(existing, ign_user);
                    }
                }
            } else {
                // Add new user entry
                if let Ok(val) = serde_json::to_value(ign_user) {
                    arr.push(val);
                }
            }
        }
    }

    // Set hostname in storage if not already set
    ensure_hostname_file(&mut config, &meta.hostname);

    // Normalize file entries: convert "inline" to "source" data URI format.
    // Ignition v3.4.0 rejects overwrite=true when contents uses "inline" instead of "source".
    normalize_file_contents(&mut config);

    serde_json::to_string_pretty(&config).unwrap_or_default()
}

/// Merge SSH keys from an IgnitionUser into an existing JSON user object.
fn merge_ssh_keys(existing: &mut Value, new_user: &IgnitionUser) {
    if let Some(new_keys) = &new_user.ssh_authorized_keys {
        let keys = existing
            .as_object_mut()
            .unwrap()
            .entry("sshAuthorizedKeys")
            .or_insert_with(|| serde_json::json!([]));

        if let Some(arr) = keys.as_array_mut() {
            let existing_set: HashSet<String> =
                arr.iter().filter_map(|k| k.as_str().map(|s| s.to_string())).collect();
            let to_add: Vec<Value> = new_keys
                .iter()
                .filter(|key| !existing_set.contains(key.as_str()))
                .map(|key| serde_json::json!(key))
                .collect();
            arr.extend(to_add);
        }
    }
}

/// Ensure /etc/hostname file exists in ignition storage.
fn ensure_hostname_file(config: &mut Value, fqdn: &str) {
    let storage = config
        .as_object_mut()
        .unwrap()
        .entry("storage")
        .or_insert_with(|| serde_json::json!({}));
    let files = storage
        .as_object_mut()
        .unwrap()
        .entry("files")
        .or_insert_with(|| serde_json::json!([]));

    if let Some(arr) = files.as_array() {
        // Check if /etc/hostname is already set
        let has_hostname = arr
            .iter()
            .any(|f| f.get("path").and_then(|p| p.as_str()) == Some("/etc/hostname"));
        if has_hostname {
            return;
        }
    }

    // URL-encode the hostname for data URI
    let encoded = url_encode(fqdn);
    if let Some(arr) = files.as_array_mut() {
        arr.push(serde_json::json!({
            "path": "/etc/hostname",
            "mode": 420,
            "overwrite": true,
            "contents": {
                "source": format!("data:,{}", encoded)
            }
        }));
    }
}

/// Normalize file contents: convert "inline" to "source" data URI format.
/// Ignition v3.4.0 rejects `overwrite: true` when contents uses "inline" without "source".
fn normalize_file_contents(config: &mut Value) {
    let files = config
        .pointer_mut("/storage/files")
        .and_then(|f| f.as_array_mut());

    if let Some(files) = files {
        for file in files.iter_mut() {
            if let Some(contents) = file.get_mut("contents") {
                if let Some(obj) = contents.as_object_mut() {
                    if let Some(inline_val) = obj.remove("inline") {
                        if let Some(text) = inline_val.as_str() {
                            let encoded = url_encode(text);
                            obj.insert("source".to_string(), serde_json::json!(format!("data:,{}", encoded)));
                        }
                    }
                }
            }
        }
    }
}

/// Generate a default Ignition v3.4.0 config from HostMetadata (no BMH base).
fn generate_ignition(meta: &HostMetadata) -> String {
    let users = build_ignition_users(meta);

    let encoded_hostname = url_encode(&meta.hostname);

    let config = IgnitionConfig {
        ignition: IgnitionMeta {
            version: "3.4.0".to_string(),
        },
        passwd: Some(IgnitionPasswd { users }),
        storage: Some(IgnitionStorage {
            files: vec![IgnitionFile {
                path: "/etc/hostname".to_string(),
                mode: Some(420), // 0644
                overwrite: Some(true),
                contents: IgnitionFileContents {
                    source: format!("data:,{}", encoded_hostname),
                },
            }],
        }),
        systemd: None,
    };

    serde_json::to_string_pretty(&config).unwrap_or_default()
}

/// Build Ignition user entries from HostMetadata.
///
/// Creates entries for:
/// 1. System users (from public_keys) - just SSH keys
/// 2. Identity users (from cloud_config) - full user creation
fn build_ignition_users(meta: &HostMetadata) -> Vec<IgnitionUser> {
    let mut users = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // System users from public_keys (root, core) - just SSH keys
    for pk in &meta.public_keys {
        seen.insert(pk.ssh_user.clone());
        users.push(IgnitionUser {
            name: pk.ssh_user.clone(),
            uid: None,
            groups: None,
            shell: None,
            home_dir: None,
            ssh_authorized_keys: Some(pk.keys.clone()),
            password_hash: None,
        });
    }

    // Identity users from cloud_config - full user creation with SSH keys
    for cc_user in &meta.cloud_config.users {
        if seen.contains(&cc_user.name) {
            // Already added as a system user, just merge keys
            if let Some(existing) = users.iter_mut().find(|u| u.name == cc_user.name) {
                if let Some(ref mut keys) = existing.ssh_authorized_keys {
                    for key in &cc_user.ssh_authorized_keys {
                        if !keys.contains(key) {
                            keys.push(key.clone());
                        }
                    }
                }
                // Also set the full user spec if this identity user IS the system user
                existing.uid = cc_user.uid.parse().ok();
                if !cc_user.groups.is_empty() {
                    existing.groups = Some(cc_user.groups.clone());
                }
                existing.shell = Some(cc_user.shell.clone());
            }
            continue;
        }

        seen.insert(cc_user.name.clone());
        let mut groups = cc_user.groups.clone();
        // Ensure sudo users are in wheel group
        if cc_user.sudo.is_some() && !groups.contains(&"wheel".to_string()) {
            groups.push("wheel".to_string());
        }

        users.push(IgnitionUser {
            name: cc_user.name.clone(),
            uid: cc_user.uid.parse().ok(),
            groups: if groups.is_empty() {
                None
            } else {
                Some(groups)
            },
            shell: Some(cc_user.shell.clone()),
            home_dir: None,
            ssh_authorized_keys: Some(cc_user.ssh_authorized_keys.clone()),
            password_hash: Some(String::new()), // empty = no password login
        });
    }

    users
}

/// Merge SSH keys and users into a base kickstart config.
fn merge_kickstart(base: &str, meta: &HostMetadata) -> String {
    let mut lines: Vec<String> = base.lines().map(|l| l.to_string()).collect();

    // Find insertion point: before %packages or %post, or at the end
    let insert_pos = lines
        .iter()
        .position(|l| l.starts_with("%packages") || l.starts_with("%post"))
        .unwrap_or(lines.len());

    let mut extra_lines = Vec::new();

    // Add hostname if not already present
    if !lines.iter().any(|l| l.starts_with("network") && l.contains("--hostname")) {
        extra_lines.push(format!(
            "network --hostname={}",
            meta.hostname
        ));
    }

    // Collect identity user names (these will be created below)
    let identity_users: std::collections::HashSet<&str> = meta
        .cloud_config
        .users
        .iter()
        .map(|u| u.name.as_str())
        .collect();

    // Add SSH keys for system users — only for users that exist on standard Linux
    // (root always exists; skip CoreOS-only users like "core" unless they're identity users)
    for pk in &meta.public_keys {
        if pk.ssh_user != "root" && !identity_users.contains(pk.ssh_user.as_str()) {
            continue;
        }
        for key in &pk.keys {
            extra_lines.push(format!("sshkey --username={} \"{}\"", pk.ssh_user, key));
        }
    }

    // Add identity users
    for cc_user in &meta.cloud_config.users {
        let groups_str = if cc_user.groups.is_empty() {
            String::new()
        } else {
            format!(" --groups={}", cc_user.groups.join(","))
        };
        extra_lines.push(format!(
            "user --name={} --uid={}{}  --shell={}",
            cc_user.name, cc_user.uid, groups_str, cc_user.shell
        ));
        for key in &cc_user.ssh_authorized_keys {
            extra_lines.push(format!("sshkey --username={} \"{}\"", cc_user.name, key));
        }
    }

    // Insert the extra lines
    if !extra_lines.is_empty() {
        extra_lines.push(String::new()); // blank line separator
        for (i, line) in extra_lines.into_iter().enumerate() {
            lines.insert(insert_pos + i, line);
        }
    }

    lines.join("\n")
}

/// Generate a default kickstart config from HostMetadata (no BMH base).
fn generate_kickstart(meta: &HostMetadata) -> String {
    let mut ks = String::new();

    ks.push_str("#version=RHEL9\n");
    ks.push_str("# Generated by CloudID\n\n");
    ks.push_str("lang en_US.UTF-8\n");
    ks.push_str("keyboard us\n");
    ks.push_str("timezone UTC --utc\n");
    ks.push_str(&format!("network --hostname={}\n", meta.hostname));
    ks.push_str("rootpw --lock\n\n");

    // SSH keys for system users
    for pk in &meta.public_keys {
        for key in &pk.keys {
            ks.push_str(&format!("sshkey --username={} \"{}\"\n", pk.ssh_user, key));
        }
    }

    // Identity users
    for cc_user in &meta.cloud_config.users {
        let groups_str = if cc_user.groups.is_empty() {
            String::new()
        } else {
            format!(" --groups={}", cc_user.groups.join(","))
        };
        ks.push_str(&format!(
            "user --name={} --uid={}{} --shell={}\n",
            cc_user.name, cc_user.uid, groups_str, cc_user.shell
        ));
        for key in &cc_user.ssh_authorized_keys {
            ks.push_str(&format!("sshkey --username={} \"{}\"\n", cc_user.name, key));
        }
    }

    ks.push_str("\nreboot\n\n");
    ks.push_str("%packages\n");
    ks.push_str("@core\n");
    ks.push_str("%end\n");

    ks
}

/// Minimal URL-encoding for data URIs (encode special chars).
fn url_encode(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => encoded.push_str("%20"),
            '%' => encoded.push_str("%25"),
            '\n' => encoded.push_str("%0A"),
            '\r' => encoded.push_str("%0D"),
            '#' => encoded.push_str("%23"),
            _ => encoded.push(c),
        }
    }
    encoded
}

// --- Template-based provisioning ---

/// Result of template resolution: either a rendered config or None (boot from local).
pub enum TemplateResult {
    /// Rendered config content with its format.
    Config { content: String, format: TemplateFormat },
    /// No template — use default behavior (identity-only generation).
    None,
}

/// Resolve and build a config for a host using the template system.
///
/// Resolution priority:
/// 1. If host completed a oneshot → return None (boot from local)
/// 2. BMH `spec.template` field
/// 3. REST API assignment (assignments.json)
/// 4. Config-based `[[templates.assignments]]` (legacy fallback)
/// 5. No match → None (default behavior)
pub async fn resolve_and_build(
    state: &AppState,
    hostname: &str,
    meta: &HostMetadata,
    bmh: Option<&BareMetalHost>,
) -> TemplateResult {
    // Step 1: Check oneshot completion
    {
        let oneshot = state.oneshot.read().await;
        if oneshot.completed.contains_key(hostname) {
            debug!(hostname, "oneshot completed, skipping template");
            return TemplateResult::None;
        }
    }

    // Step 2-4: Resolve template reference
    let template_ref = resolve_template_ref(state, hostname, bmh).await;
    let (image_type, template_name) = match template_ref {
        Some(r) => r,
        None => return TemplateResult::None,
    };

    // Load the template
    let loaded = match state.template_store.get(&image_type, &template_name).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            debug!(image_type, template_name, hostname, "template not found on disk");
            return TemplateResult::None;
        }
        Err(e) => {
            tracing::warn!(image_type, template_name, hostname, error = %e, "failed to load template");
            return TemplateResult::None;
        }
    };

    // Substitute variables
    let content = templates::substitute_variables(
        &loaded.content,
        &meta.hostname,
        &meta.local_ipv4,
        &meta.instance_id,
        &meta.availability_zone,
        &state.config.metadata.domain_suffix,
        &loaded.name,
    );

    // Merge SSH keys based on format
    let final_content = match loaded.format {
        TemplateFormat::Ignition => {
            match serde_json::from_str::<Value>(&content) {
                Ok(base_json) => merge_ignition(&base_json, meta),
                Err(_) => {
                    tracing::warn!(image_type, template_name = loaded.name, "template is not valid JSON, serving as-is");
                    content
                }
            }
        }
        TemplateFormat::Kickstart => merge_kickstart(&content, meta),
        TemplateFormat::CloudConfig => {
            // For cloud-config, just return the substituted content
            // (SSH keys are already in the EC2 metadata path)
            content
        }
    };

    info!(hostname, image_type, template = loaded.name, mode = ?loaded.mode, "serving template config");

    TemplateResult::Config {
        content: final_content,
        format: loaded.format,
    }
}

/// Resolve which template applies to a host.
/// Returns (image_type, template_filename) or None.
async fn resolve_template_ref(
    state: &AppState,
    hostname: &str,
    bmh: Option<&BareMetalHost>,
) -> Option<(String, String)> {
    // Priority 2: BMH spec.template field
    if let Some(bmh) = bmh {
        if let Some(ref tpl) = bmh.spec.template {
            if !tpl.is_empty() {
                return Some(parse_template_ref(tpl, bmh));
            }
        }
    }

    // Priority 3: REST API assignment
    {
        let assignments = state.assignments.read().await;
        if let Some(asgn) = assignments.assignments.get(hostname) {
            return Some((asgn.image_type.clone(), asgn.template.clone()));
        }
    }

    // Priority 4: Config-based assignments
    for rule in &state.config.templates.assignments {
        if rule.hosts.iter().any(|h| h == hostname || h == "*") {
            return Some(parse_config_template_ref(&rule.template));
        }
    }

    None
}

/// Parse a template reference like "fcos/agent-runner.ign.json" or "agent-runner.ign.json".
/// If no slash, use the BMH image type to derive it.
fn parse_template_ref(tpl_ref: &str, bmh: &BareMetalHost) -> (String, String) {
    if let Some(pos) = tpl_ref.find('/') {
        let image_type = &tpl_ref[..pos];
        let name = &tpl_ref[pos + 1..];
        (image_type.to_string(), name.to_string())
    } else {
        // Derive image type from BMH image field
        let image_type = extract_image_type(&bmh.spec.image);
        (image_type, tpl_ref.to_string())
    }
}

/// Parse a config-based template reference (always "image_type/name" format).
fn parse_config_template_ref(tpl_ref: &str) -> (String, String) {
    if let Some(pos) = tpl_ref.find('/') {
        let image_type = &tpl_ref[..pos];
        let name = &tpl_ref[pos + 1..];
        (image_type.to_string(), name.to_string())
    } else {
        ("default".to_string(), tpl_ref.to_string())
    }
}

/// Extract the base image type from a BMH image string.
/// e.g., "fcos-44" -> "fcos", "fedora-9" -> "fedora", "ubuntu-24.04" -> "ubuntu"
pub fn extract_image_type(image: &str) -> String {
    // Strip trailing version: everything after the last hyphen that starts with a digit
    if let Some(pos) = image.rfind('-') {
        let after = &image[pos + 1..];
        if after.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            return image[..pos].to_string();
        }
    }
    image.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CloudConfig, CloudConfigUser, PublicKeyEntry};

    fn make_metadata() -> HostMetadata {
        HostMetadata {
            instance_id: "server1".to_string(),
            hostname: "server1.g10.lo".to_string(),
            local_hostname: "server1".to_string(),
            local_ipv4: "192.168.10.10".to_string(),
            availability_zone: "gt".to_string(),
            public_keys: vec![
                PublicKeyEntry {
                    ssh_user: "core".to_string(),
                    keys: vec!["ssh-rsa AAAA_TEST core-key".to_string()],
                },
                PublicKeyEntry {
                    ssh_user: "root".to_string(),
                    keys: vec!["ssh-rsa AAAA_TEST root-key".to_string()],
                },
            ],
            user_data: String::new(),
            cloud_config: CloudConfig {
                users: vec![CloudConfigUser {
                    name: "gwest".to_string(),
                    uid: "1000".to_string(),
                    groups: vec!["wheel".to_string()],
                    shell: "/bin/bash".to_string(),
                    sudo: Some("ALL=(ALL) NOPASSWD:ALL".to_string()),
                    ssh_authorized_keys: vec!["ssh-rsa AAAA_TEST gwest-key".to_string()],
                }],
            },
        }
    }

    #[test]
    fn test_generate_ignition() {
        let meta = make_metadata();
        let output = generate_ignition(&meta);
        let parsed: Value = serde_json::from_str(&output).expect("valid JSON");

        assert_eq!(parsed["ignition"]["version"], "3.4.0");
        let users = parsed["passwd"]["users"].as_array().unwrap();
        assert!(users.iter().any(|u| u["name"] == "core"));
        assert!(users.iter().any(|u| u["name"] == "root"));
        assert!(users.iter().any(|u| u["name"] == "gwest"));

        // Check hostname file
        let files = parsed["storage"]["files"].as_array().unwrap();
        assert!(files.iter().any(|f| f["path"] == "/etc/hostname"));
    }

    #[test]
    fn test_merge_ignition() {
        let meta = make_metadata();
        let base = serde_json::json!({
            "ignition": {"version": "3.4.0"},
            "passwd": {
                "users": [
                    {
                        "name": "core",
                        "sshAuthorizedKeys": ["ssh-rsa EXISTING_KEY existing"]
                    }
                ]
            }
        });

        let output = merge_ignition(&base, &meta);
        let parsed: Value = serde_json::from_str(&output).expect("valid JSON");

        let users = parsed["passwd"]["users"].as_array().unwrap();
        // core should have both existing and new keys
        let core_user = users.iter().find(|u| u["name"] == "core").unwrap();
        let core_keys = core_user["sshAuthorizedKeys"].as_array().unwrap();
        assert!(core_keys.len() >= 2); // existing + new

        // gwest should be added
        assert!(users.iter().any(|u| u["name"] == "gwest"));
    }

    #[test]
    fn test_generate_kickstart() {
        let meta = make_metadata();
        let output = generate_kickstart(&meta);

        assert!(output.contains("#version=RHEL9"));
        assert!(output.contains("network --hostname=server1.g10.lo"));
        assert!(output.contains("sshkey --username=root"));
        assert!(output.contains("sshkey --username=core"));
        assert!(output.contains("user --name=gwest"));
        assert!(output.contains("%packages"));
    }

    #[test]
    fn test_merge_kickstart() {
        let meta = make_metadata();
        let base = "#version=RHEL9\nlang en_US.UTF-8\n%packages\n@core\n%end\n";

        let output = merge_kickstart(base, &meta);

        assert!(output.contains("sshkey --username=root"));
        assert!(output.contains("user --name=gwest"));
        // SSH keys should be before %packages
        let ssh_pos = output.find("sshkey").unwrap();
        let pkg_pos = output.find("%packages").unwrap();
        assert!(ssh_pos < pkg_pos);
    }

    #[test]
    fn test_extract_image_type() {
        assert_eq!(extract_image_type("fcos-44"), "fcos");
        assert_eq!(extract_image_type("fedora-9"), "fedora");
        assert_eq!(extract_image_type("ubuntu-24.04"), "ubuntu");
        assert_eq!(extract_image_type("fcos"), "fcos");
        assert_eq!(extract_image_type("my-custom-image-3"), "my-custom-image");
    }

    #[test]
    fn test_parse_config_template_ref() {
        let (it, name) = parse_config_template_ref("fcos/agent-runner.ign.json");
        assert_eq!(it, "fcos");
        assert_eq!(name, "agent-runner.ign.json");

        let (it, name) = parse_config_template_ref("just-a-name");
        assert_eq!(it, "default");
        assert_eq!(name, "just-a-name");
    }
}
