use crate::cache::AppState;
use crate::model::{BareMetalHostList, DhcpLease, WatchEvent};
use anyhow::Result;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

/// Start the BMH watcher. Connects to mkube, lists existing BMH objects,
/// then watches for changes via streaming HTTP.
///
/// Also periodically refreshes DHCP lease sources for additional IP mappings.
///
/// Self-healing: on disconnect, reconnects with exponential backoff.
pub async fn start(state: Arc<AppState>) {
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);

    loop {
        info!(url = %state.config.mkube.url, "connecting to mkube BMH watcher");

        match run_watcher(&state).await {
            Ok(()) => {
                info!("BMH watcher exited cleanly");
                backoff = Duration::from_secs(1);
            }
            Err(e) => {
                error!(error = %e, "BMH watcher failed");
            }
        }

        warn!(backoff_secs = backoff.as_secs(), "reconnecting to mkube");
        sleep(backoff).await;
        backoff = (backoff * 2).min(max_backoff);
    }
}

async fn run_watcher(state: &Arc<AppState>) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(10))
        .build()?;

    // Initial list of all BMH objects across configured namespaces
    load_all_bmh(&client, state).await?;

    // Load DHCP leases for additional IP mappings
    load_dhcp_leases(&client, state).await;

    // Rebuild cache after initial load
    state.rebuild_cache().await;

    // Watch for changes across all namespaces concurrently
    // Also run a periodic full refresh as a consistency check
    let state_refresh = state.clone();
    let client_refresh = client.clone();

    tokio::select! {
        r = watch_all_namespaces(&client, state) => r,
        _ = periodic_refresh(&client_refresh, &state_refresh) => Ok(()),
    }
}

async fn load_all_bmh(client: &reqwest::Client, state: &Arc<AppState>) -> Result<()> {
    let mut bmh = state.bmh.write().await;
    bmh.ip_to_hostname.clear();
    bmh.host_labels.clear();
    bmh.host_namespace.clear();
    bmh.hosts.clear();

    for ns in &state.config.mkube.bmh_namespaces {
        let url = format!(
            "{}/api/v1/namespaces/{}/baremetalhosts",
            state.config.mkube.url, ns
        );

        match client.get(&url).send().await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    warn!(ns, status = %resp.status(), "failed to list BMH");
                    continue;
                }

                match resp.json::<BareMetalHostList>().await {
                    Ok(list) => {
                        for host in list.items {
                            apply_bmh_to_state(&host, &mut bmh);
                        }
                        info!(ns, hosts = bmh.ip_to_hostname.len(), "loaded BMH objects");
                    }
                    Err(e) => {
                        warn!(ns, error = %e, "failed to parse BMH list");
                    }
                }
            }
            Err(e) => {
                warn!(ns, error = %e, "failed to connect to mkube for BMH list");
            }
        }
    }

    info!(
        total_hosts = bmh.ip_to_hostname.len(),
        "BMH initial load complete"
    );
    Ok(())
}

fn apply_bmh_to_state(
    host: &crate::model::BareMetalHost,
    bmh: &mut crate::cache::BmhState,
) {
    // Prefer status.ip (live IP) over spec.ip (configured IP)
    let ip_str = host
        .status
        .as_ref()
        .and_then(|s| if s.ip.is_empty() { None } else { Some(&s.ip) })
        .or(if host.spec.ip.is_empty() {
            None
        } else {
            Some(&host.spec.ip)
        });

    let hostname = if host.spec.hostname.is_empty() {
        &host.metadata.name
    } else {
        &host.spec.hostname
    };

    if let Some(ip_str) = ip_str {
        match ip_str.parse::<IpAddr>() {
            Ok(ip) => {
                debug!(ip = %ip, hostname, "BMH IP mapping");
                bmh.ip_to_hostname.insert(ip, hostname.to_string());
                bmh.host_labels
                    .insert(hostname.to_string(), host.metadata.labels.clone());
                bmh.host_namespace
                    .insert(hostname.to_string(), host.metadata.namespace.clone());
                // Store full BMH data for provisioning config generation
                bmh.hosts.insert(hostname.to_string(), host.clone());
            }
            Err(e) => {
                warn!(ip = ip_str, hostname, error = %e, "invalid IP in BMH");
            }
        }
    }
}

async fn load_dhcp_leases(client: &reqwest::Client, state: &Arc<AppState>) {
    for source in &state.config.metadata.dhcp_sources {
        match client
            .get(source)
            .timeout(Duration::from_secs(5))
            .send()
            .await
        {
            Ok(resp) => {
                if !resp.status().is_success() {
                    warn!(source, status = %resp.status(), "failed to fetch DHCP leases");
                    continue;
                }

                match resp.json::<Vec<DhcpLease>>().await {
                    Ok(leases) => {
                        let mut bmh = state.bmh.write().await;
                        let mut added = 0;
                        for lease in leases {
                            if lease.hostname.is_empty() {
                                continue;
                            }
                            if let Ok(ip) = lease.ip.parse::<IpAddr>() {
                                // Only add if not already known from BMH
                                use std::collections::hash_map::Entry;
                                if let Entry::Vacant(e) = bmh.ip_to_hostname.entry(ip) {
                                    e.insert(lease.hostname.clone());
                                    added += 1;
                                }
                            }
                        }
                        info!(source, added, "loaded DHCP leases");
                    }
                    Err(e) => {
                        warn!(source, error = %e, "failed to parse DHCP leases");
                    }
                }
            }
            Err(e) => {
                warn!(source, error = %e, "failed to connect to DHCP source");
            }
        }
    }
}

async fn watch_all_namespaces(client: &reqwest::Client, state: &Arc<AppState>) -> Result<()> {
    // Watch all configured namespaces. If any watch fails, we return the error
    // and the outer loop will reconnect everything.
    let mut handles = Vec::new();

    for ns in &state.config.mkube.bmh_namespaces {
        let client = client.clone();
        let state = state.clone();
        let ns = ns.clone();

        handles.push(tokio::spawn(async move {
            watch_namespace(&client, &state, &ns).await
        }));
    }

    // Wait for any watch to fail
    let (result, _, remaining) = futures::future::select_all(handles).await;
    // Cancel remaining watches
    for handle in remaining {
        handle.abort();
    }

    result??;
    Ok(())
}

async fn watch_namespace(
    client: &reqwest::Client,
    state: &Arc<AppState>,
    ns: &str,
) -> Result<()> {
    let url = format!(
        "{}/api/v1/namespaces/{}/baremetalhosts?watch=true",
        state.config.mkube.url, ns
    );

    info!(ns, "starting BMH watch");

    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(3600))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("BMH watch returned status {}", resp.status());
    }

    // Read newline-delimited JSON events
    let mut bytes = resp.bytes_stream();
    let mut buffer = String::new();

    use futures::StreamExt;
    while let Some(chunk) = bytes.next().await {
        let chunk = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Process complete lines
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer = buffer[pos + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<WatchEvent>(&line) {
                Ok(event) => {
                    handle_watch_event(&event, state).await;
                }
                Err(e) => {
                    warn!(ns, error = %e, "failed to parse BMH watch event");
                }
            }
        }
    }

    Ok(())
}

async fn handle_watch_event(event: &WatchEvent, state: &Arc<AppState>) {
    let hostname = if event.object.spec.hostname.is_empty() {
        &event.object.metadata.name
    } else {
        &event.object.spec.hostname
    };

    match event.event_type.as_str() {
        "ADDED" | "MODIFIED" => {
            let mut bmh = state.bmh.write().await;
            apply_bmh_to_state(&event.object, &mut bmh);
            drop(bmh);
            info!(
                hostname,
                event = event.event_type,
                "BMH updated via watch"
            );
            state.rebuild_cache().await;
        }
        "DELETED" => {
            let mut bmh = state.bmh.write().await;
            // Remove all IP mappings for this hostname
            bmh.ip_to_hostname.retain(|_, h| h != hostname);
            bmh.host_labels.remove(hostname);
            bmh.host_namespace.remove(hostname);
            bmh.hosts.remove(hostname);
            drop(bmh);
            info!(hostname, "BMH deleted via watch");
            state.rebuild_cache().await;
        }
        other => {
            warn!(event_type = other, hostname, "unknown BMH watch event type");
        }
    }
}

/// Periodic full refresh as a consistency check.
/// Re-lists all BMH objects and DHCP leases to catch any missed events.
async fn periodic_refresh(client: &reqwest::Client, state: &Arc<AppState>) {
    let interval = Duration::from_secs(state.config.metadata.cache_rebuild_interval_secs);

    loop {
        sleep(interval).await;
        debug!("periodic BMH refresh");

        if let Err(e) = load_all_bmh(client, state).await {
            warn!(error = %e, "periodic BMH refresh failed");
        }
        load_dhcp_leases(client, state).await;
        state.rebuild_cache().await;
    }
}
