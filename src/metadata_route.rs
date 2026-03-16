use crate::config::NetworkConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

const METADATA_IP: &str = "169.254.169.254/32";
const MANAGED_BY: &str = "cloudid";

#[derive(Debug, Serialize)]
struct RouteRequest {
    destination: String,
    gateway: String,
    managed_by: String,
}

#[derive(Debug, Deserialize)]
struct RouteEntry {
    #[allow(dead_code)]
    id: String,
    destination: String,
    #[allow(dead_code)]
    gateway: String,
    #[allow(dead_code)]
    managed_by: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RoutesResponse {
    routes: Vec<RouteEntry>,
}

#[derive(Debug, Deserialize)]
struct DhcpPool {
    id: String,
    #[allow(dead_code)]
    subnet: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PoolsResponse {
    pools: Vec<DhcpPool>,
}

/// Ensure the metadata DHCP route (169.254.169.254/32 via gateway) is configured
/// on each data network's MicroDNS. Runs on startup and periodically.
pub async fn start(networks: HashMap<String, NetworkConfig>) {
    if networks.is_empty() {
        info!("no data networks configured, skipping metadata route setup");
        return;
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .expect("failed to build HTTP client");

    // Initial setup
    for (name, net) in &networks {
        ensure_route(&client, name, net).await;
    }

    // Periodic verification (self-healing)
    let interval = Duration::from_secs(300); // every 5 minutes
    loop {
        sleep(interval).await;
        for (name, net) in &networks {
            ensure_route(&client, name, net).await;
        }
    }
}

/// Ensure the metadata route exists on a network's MicroDNS DHCP pools.
async fn ensure_route(client: &reqwest::Client, network: &str, net: &NetworkConfig) {
    let pools = match list_pools(client, &net.dns).await {
        Ok(p) => p,
        Err(e) => {
            warn!(network, dns = %net.dns, error = %e, "failed to list DHCP pools");
            return;
        }
    };

    if pools.is_empty() {
        warn!(network, "no DHCP pools found on MicroDNS");
        return;
    }

    for pool in &pools {
        match check_route(client, &net.dns, &pool.id).await {
            Ok(true) => {
                info!(network, pool = %pool.id, "metadata route already configured");
            }
            Ok(false) => {
                match add_route(client, &net.dns, &pool.id, &net.gateway).await {
                    Ok(()) => {
                        info!(network, pool = %pool.id, gateway = %net.gateway, "metadata route added");
                    }
                    Err(e) => {
                        error!(network, pool = %pool.id, error = %e, "failed to add metadata route");
                    }
                }
            }
            Err(e) => {
                warn!(network, pool = %pool.id, error = %e, "failed to check metadata route");
            }
        }
    }
}

async fn list_pools(client: &reqwest::Client, dns_url: &str) -> anyhow::Result<Vec<DhcpPool>> {
    let url = format!("{}/api/v1/dhcp/pools", dns_url);
    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        anyhow::bail!("GET {} returned {}", url, resp.status());
    }

    let body: PoolsResponse = resp.json().await?;
    Ok(body.pools)
}

async fn check_route(
    client: &reqwest::Client,
    dns_url: &str,
    pool_id: &str,
) -> anyhow::Result<bool> {
    let url = format!("{}/api/v1/dhcp/pools/{}/routes", dns_url, pool_id);
    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        anyhow::bail!("GET {} returned {}", url, resp.status());
    }

    let body: RoutesResponse = resp.json().await?;
    Ok(body.routes.iter().any(|r| r.destination == METADATA_IP))
}

async fn add_route(
    client: &reqwest::Client,
    dns_url: &str,
    pool_id: &str,
    gateway: &str,
) -> anyhow::Result<()> {
    let url = format!("{}/api/v1/dhcp/pools/{}/routes", dns_url, pool_id);
    let req = RouteRequest {
        destination: METADATA_IP.to_string(),
        gateway: gateway.to_string(),
        managed_by: MANAGED_BY.to_string(),
    };

    let resp = client.post(&url).json(&req).send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("POST {} returned {}: {}", url, status, body);
    }

    Ok(())
}
