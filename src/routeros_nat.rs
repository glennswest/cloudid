use crate::config::RouterOsConfig;
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Duration};
use tracing::{debug, info, warn};

const IMDS_DST: &str = "169.254.169.254";
const IMDS_PORT: &str = "80";
const MANAGED_COMMENT: &str = "IMDS redirect to cloudid (managed)";

/// RouterOS NAT rule as returned by the REST API.
#[derive(Debug, Deserialize)]
struct NatRule {
    #[serde(rename = ".id")]
    id: String,
    #[serde(default)]
    chain: String,
    #[serde(default, rename = "dst-address")]
    dst_address: String,
    #[serde(default)]
    protocol: String,
    #[serde(default, rename = "dst-port")]
    dst_port: String,
    #[serde(default)]
    action: String,
    #[serde(default, rename = "to-addresses")]
    to_addresses: String,
    #[serde(default, rename = "to-ports")]
    to_ports: String,
    #[serde(default)]
    #[allow(dead_code)]
    comment: String,
}

/// Payload for creating a new NAT rule.
#[derive(Debug, Serialize)]
struct CreateNatRule {
    chain: String,
    #[serde(rename = "dst-address")]
    dst_address: String,
    protocol: String,
    #[serde(rename = "dst-port")]
    dst_port: String,
    action: String,
    #[serde(rename = "to-addresses")]
    to_addresses: String,
    #[serde(rename = "to-ports")]
    to_ports: String,
    comment: String,
}

/// Payload for updating an existing NAT rule.
#[derive(Debug, Serialize)]
struct UpdateNatRule {
    #[serde(rename = "to-addresses")]
    to_addresses: String,
    #[serde(rename = "to-ports")]
    to_ports: String,
}

/// Start the RouterOS NAT manager background loop.
///
/// On startup (with retry) and then every 60s, ensures a DST-NAT rule
/// exists on the router that redirects 169.254.169.254:80 to cloudid.
pub async fn start(ros_config: RouterOsConfig, cloudid_ip: String, cloudid_port: String) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .expect("failed to build HTTP client");

    // Initial setup with exponential backoff
    let mut backoff = Duration::from_secs(2);
    loop {
        match ensure_nat_rule(&client, &ros_config, &cloudid_ip, &cloudid_port).await {
            Ok(()) => break,
            Err(e) => {
                warn!(error = %e, backoff_secs = backoff.as_secs(), "RouterOS NAT check failed, retrying");
                sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        }
    }

    // Periodic verification every 60s
    let interval = Duration::from_secs(60);
    loop {
        sleep(interval).await;
        if let Err(e) = ensure_nat_rule(&client, &ros_config, &cloudid_ip, &cloudid_port).await {
            warn!(error = %e, "periodic RouterOS NAT check failed");
        }
    }
}

/// Check if the IMDS DST-NAT rule exists, create or update as needed.
async fn ensure_nat_rule(
    client: &reqwest::Client,
    config: &RouterOsConfig,
    cloudid_ip: &str,
    cloudid_port: &str,
) -> anyhow::Result<()> {
    let url = format!("{}/ip/firewall/nat", config.rest_url);
    let resp = client
        .get(&url)
        .basic_auth(&config.user, Some(&config.password))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("GET {} returned {}: {}", url, status, body);
    }

    let rules: Vec<NatRule> = resp.json().await?;

    // Find existing managed rule by comment or by matching dst-address + dst-port
    let existing = rules.iter().find(|r| {
        r.chain == "dstnat"
            && r.dst_address == IMDS_DST
            && r.protocol == "tcp"
            && r.dst_port == IMDS_PORT
            && r.action == "dst-nat"
    });

    match existing {
        Some(rule) => {
            // Rule exists — check if to-addresses and to-ports match
            if rule.to_addresses == cloudid_ip && rule.to_ports == cloudid_port {
                debug!("RouterOS IMDS NAT rule present and correct");
                Ok(())
            } else {
                // Update the rule
                info!(
                    rule_id = %rule.id,
                    old_addr = %rule.to_addresses,
                    old_port = %rule.to_ports,
                    new_addr = %cloudid_ip,
                    new_port = %cloudid_port,
                    "updating RouterOS IMDS NAT rule"
                );
                let patch_url = format!("{}/{}", url, rule.id);
                let patch = UpdateNatRule {
                    to_addresses: cloudid_ip.to_string(),
                    to_ports: cloudid_port.to_string(),
                };
                let resp = client
                    .patch(&patch_url)
                    .basic_auth(&config.user, Some(&config.password))
                    .json(&patch)
                    .send()
                    .await?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    anyhow::bail!("PATCH {} returned {}: {}", patch_url, status, body);
                }

                info!("RouterOS IMDS NAT rule updated");
                Ok(())
            }
        }
        None => {
            // Create the rule
            info!(
                to_addr = %cloudid_ip,
                to_port = %cloudid_port,
                "creating RouterOS IMDS NAT rule"
            );
            let create = CreateNatRule {
                chain: "dstnat".to_string(),
                dst_address: IMDS_DST.to_string(),
                protocol: "tcp".to_string(),
                dst_port: IMDS_PORT.to_string(),
                action: "dst-nat".to_string(),
                to_addresses: cloudid_ip.to_string(),
                to_ports: cloudid_port.to_string(),
                comment: MANAGED_COMMENT.to_string(),
            };
            let resp = client
                .put(&url)
                .basic_auth(&config.user, Some(&config.password))
                .json(&create)
                .send()
                .await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("PUT {} returned {}: {}", url, status, body);
            }

            info!("RouterOS IMDS NAT rule created");
            Ok(())
        }
    }
}
