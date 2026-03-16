use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

const METADATA_IP: &str = "169.254.169.254/32";
const MANAGED_BY: &str = "cloudid";

// --- mkube Network types ---

#[derive(Debug, Deserialize)]
struct NetworkList {
    items: Vec<Network>,
}

#[derive(Debug, Deserialize)]
struct Network {
    metadata: NetworkMeta,
    spec: NetworkSpec,
}

#[derive(Debug, Deserialize)]
struct NetworkMeta {
    name: String,
}

#[derive(Debug, Deserialize)]
struct NetworkSpec {
    #[serde(rename = "type")]
    net_type: String,
    gateway: String,
    #[serde(default)]
    dns: Option<NetworkDns>,
    #[serde(default)]
    dhcp: Option<NetworkDhcp>,
}

#[derive(Debug, Deserialize)]
struct NetworkDns {
    endpoint: String,
}

#[derive(Debug, Deserialize)]
struct NetworkDhcp {
    #[serde(default)]
    enabled: bool,
}

/// Discover data network names from mkube.
/// Returns the list of network names where type == "data".
pub async fn discover_data_networks(mkube_url: &str) -> anyhow::Result<Vec<String>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()?;

    let url = format!("{}/api/v1/networks", mkube_url);
    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        anyhow::bail!("GET {} returned {}", url, resp.status());
    }

    let list: NetworkList = resp.json().await?;
    let names: Vec<String> = list
        .items
        .iter()
        .filter(|n| n.spec.net_type == "data")
        .map(|n| n.metadata.name.clone())
        .collect();

    Ok(names)
}

// --- MicroDNS route types ---

#[derive(Debug, Serialize)]
struct RouteRequest {
    destination: String,
    gateway: String,
    managed_by: String,
}

#[derive(Debug, Deserialize)]
struct RouteEntry {
    destination: String,
}

#[derive(Debug, Deserialize)]
struct RoutesResponse {
    routes: Vec<RouteEntry>,
}

#[derive(Debug, Deserialize)]
struct DhcpPool {
    id: String,
}

#[derive(Debug, Deserialize)]
struct PoolsResponse {
    pools: Vec<DhcpPool>,
}

/// Discover data networks from mkube and ensure the DHCP metadata route
/// (169.254.169.254/32 via gateway) is configured on each via MicroDNS.
pub async fn start(mkube_url: String) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .expect("failed to build HTTP client");

    // Initial setup with retry
    let mut backoff = Duration::from_secs(2);
    loop {
        match discover_and_configure(&client, &mkube_url).await {
            Ok(count) => {
                info!(networks = count, "metadata routes configured");
                break;
            }
            Err(e) => {
                warn!(error = %e, backoff_secs = backoff.as_secs(), "metadata route setup failed, retrying");
                sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        }
    }

    // Periodic verification (self-healing)
    let interval = Duration::from_secs(300);
    loop {
        sleep(interval).await;
        if let Err(e) = discover_and_configure(&client, &mkube_url).await {
            warn!(error = %e, "periodic metadata route check failed");
        }
    }
}

/// Discover data networks from mkube and ensure routes on each.
/// Returns the number of networks configured.
async fn discover_and_configure(
    client: &reqwest::Client,
    mkube_url: &str,
) -> anyhow::Result<usize> {
    let url = format!("{}/api/v1/networks", mkube_url);
    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        anyhow::bail!("GET {} returned {}", url, resp.status());
    }

    let list: NetworkList = resp.json().await?;
    let mut count = 0;

    for net in &list.items {
        // Only data networks with DHCP enabled need the metadata route
        let dhcp_enabled = net.spec.dhcp.as_ref().map(|d| d.enabled).unwrap_or(false);
        if net.spec.net_type != "data" || !dhcp_enabled {
            debug!(network = %net.metadata.name, net_type = %net.spec.net_type, "skipping non-data network");
            continue;
        }

        let dns_endpoint = match &net.spec.dns {
            Some(dns) => &dns.endpoint,
            None => {
                warn!(network = %net.metadata.name, "data network has no DNS endpoint");
                continue;
            }
        };

        ensure_route(client, &net.metadata.name, dns_endpoint, &net.spec.gateway).await;
        count += 1;
    }

    Ok(count)
}

/// Ensure the metadata route exists on a network's MicroDNS DHCP pools.
async fn ensure_route(
    client: &reqwest::Client,
    network: &str,
    dns_endpoint: &str,
    gateway: &str,
) {
    let pools = match list_pools(client, dns_endpoint).await {
        Ok(p) => p,
        Err(e) => {
            warn!(network, dns = %dns_endpoint, error = %e, "failed to list DHCP pools");
            return;
        }
    };

    if pools.is_empty() {
        warn!(network, "no DHCP pools found on MicroDNS");
        return;
    }

    for pool in &pools {
        match check_route(client, dns_endpoint, &pool.id).await {
            Ok(true) => {
                debug!(network, pool = %pool.id, "metadata route present");
            }
            Ok(false) => {
                match add_route(client, dns_endpoint, &pool.id, gateway).await {
                    Ok(()) => {
                        info!(network, pool = %pool.id, gateway, "metadata route added");
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
