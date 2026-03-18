use crate::cache::{AppState, ContainerInfo, ContainerState};
use crate::model::{K8sNamespaceList, K8sPodList};
use std::net::IpAddr;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

/// Start the container watcher. Polls mkube for pod IPs and namespace owners,
/// then rebuilds the metadata cache to serve container identity.
///
/// Self-healing: on failure, reconnects with exponential backoff.
pub async fn start(state: Arc<AppState>) {
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);

    loop {
        info!(url = %state.config.mkube.url, "connecting to mkube container watcher");

        match run_watcher(&state).await {
            Ok(()) => {
                info!("container watcher exited cleanly");
                backoff = Duration::from_secs(1);
            }
            Err(e) => {
                error!(error = %e, "container watcher failed");
            }
        }

        warn!(backoff_secs = backoff.as_secs(), "reconnecting container watcher");
        sleep(backoff).await;
        backoff = (backoff * 2).min(max_backoff);
    }
}

async fn run_watcher(state: &Arc<AppState>) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()?;

    // Initial load
    load_container_state(&client, state).await?;
    state.rebuild_cache().await;

    // Periodic refresh loop
    let interval = Duration::from_secs(state.config.metadata.cache_rebuild_interval_secs);
    loop {
        sleep(interval).await;
        debug!("periodic container refresh");

        if let Err(e) = load_container_state(&client, state).await {
            warn!(error = %e, "periodic container refresh failed");
        }
        state.rebuild_cache().await;
    }
}

async fn load_container_state(
    client: &reqwest::Client,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let mkube_url = &state.config.mkube.url;

    // Load namespace owners
    let namespace_owners = load_namespaces(client, mkube_url).await?;
    let ns_count = namespace_owners.len();

    // Load pod IPs
    let ip_to_container = load_pods(client, mkube_url).await?;
    let pod_count = ip_to_container.len();

    // Update state
    let mut containers = state.containers.write().await;
    *containers = ContainerState {
        ip_to_container,
        namespace_owners,
    };

    info!(
        namespaces_with_owner = ns_count,
        container_ips = pod_count,
        "container state loaded"
    );

    Ok(())
}

/// Load namespaces from mkube, extract vkube.io/owner annotation.
async fn load_namespaces(
    client: &reqwest::Client,
    mkube_url: &str,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    let url = format!("{}/api/v1/namespaces", mkube_url);

    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("namespace list returned status {}", resp.status());
    }

    let list: K8sNamespaceList = resp.json().await?;
    let mut owners = std::collections::HashMap::new();

    for ns in list.items {
        if let Some(owner) = ns.metadata.annotations.get("vkube.io/owner") {
            if !owner.is_empty() {
                owners.insert(ns.metadata.name.clone(), owner.clone());
                debug!(namespace = ns.metadata.name, owner, "namespace owner");
            }
        }
    }

    Ok(owners)
}

/// Load pods from mkube, extract pod IPs and container info.
async fn load_pods(
    client: &reqwest::Client,
    mkube_url: &str,
) -> anyhow::Result<std::collections::HashMap<IpAddr, ContainerInfo>> {
    let url = format!("{}/api/v1/pods", mkube_url);

    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("pod list returned status {}", resp.status());
    }

    let list: K8sPodList = resp.json().await?;
    let mut ip_map = std::collections::HashMap::new();

    for pod in list.items {
        let pod_ip = pod
            .status
            .as_ref()
            .map(|s| s.pod_ip.as_str())
            .unwrap_or("");

        if pod_ip.is_empty() {
            continue;
        }

        let ip: IpAddr = match pod_ip.parse() {
            Ok(ip) => ip,
            Err(e) => {
                warn!(
                    pod = pod.metadata.name,
                    ip = pod_ip,
                    error = %e,
                    "invalid pod IP"
                );
                continue;
            }
        };

        // Container name: first container status name, or "main"
        let container_name = pod
            .status
            .as_ref()
            .and_then(|s| s.container_statuses.first())
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "main".to_string());

        // Hostname format: container.pod (e.g. "main.cloudid")
        let hostname = format!("{}.{}", container_name, pod.metadata.name);

        ip_map.insert(
            ip,
            ContainerInfo {
                namespace: pod.metadata.namespace.clone(),
                pod_name: pod.metadata.name.clone(),
                container_name,
                hostname,
            },
        );

        debug!(
            pod = pod.metadata.name,
            namespace = pod.metadata.namespace,
            %ip,
            "container IP mapping"
        );
    }

    Ok(ip_map)
}
