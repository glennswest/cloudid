use crate::cache::AppState;
use crate::provision;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{debug, info, warn};

type AppResponse = Result<String, StatusCode>;

fn get_source_ip(addr: &SocketAddr) -> std::net::IpAddr {
    addr.ip()
}

// --- IMDSv2 token endpoint ---

/// PUT /latest/api/token — generate and return a per-host IMDSv2 session token.
/// Cloud-init/afterburn request this first for IMDSv2.
pub async fn api_token(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let ip = get_source_ip(&addr);
    let ttl = headers
        .get("x-aws-ec2-metadata-token-ttl-seconds")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(21600)
        .min(21600); // Cap at 6 hours

    let token = state.generate_imds_token(ip, ttl);
    info!(%ip, ttl, "IMDSv2 token issued");

    (StatusCode::OK, token)
}

// --- Directory listings ---

pub async fn root() -> &'static str {
    "latest\n"
}

pub async fn latest() -> &'static str {
    "dynamic\nmeta-data\nuser-data\n"
}

pub async fn dynamic_index() -> &'static str {
    "instance-identity/\n"
}

pub async fn instance_identity_index() -> &'static str {
    "document\n"
}

pub async fn meta_data_index(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    let has_meta = state.get_metadata(&ip).is_some();

    // Return full listing with optional fields
    let mut listing = String::from(
        "ami-id\n\
         instance-id\n\
         instance-type\n\
         hostname\n\
         local-hostname\n\
         local-ipv4\n\
         mac\n\
         placement/\n\
         public-keys/\n\
         services/\n",
    );
    if has_meta {
        listing.push_str("network/\n");
    }
    Ok(listing)
}

pub async fn placement_index() -> &'static str {
    "availability-zone\nregion\n"
}

pub async fn services_index() -> &'static str {
    "domain\npartition\n"
}

pub async fn services_domain() -> &'static str {
    "amazonaws.com"
}

pub async fn services_partition() -> &'static str {
    "aws"
}

pub async fn network_index() -> &'static str {
    "interfaces/\n"
}

pub async fn network_interfaces_index() -> &'static str {
    "macs/\n"
}

// --- Instance identity document ---

pub async fn instance_identity_document(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Result<(StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String), StatusCode>
{
    let ip = get_source_ip(&addr);
    let meta = lookup_or_404(&state, &ip)?;

    let doc = serde_json::json!({
        "accountId": "000000000000",
        "architecture": "x86_64",
        "availabilityZone": meta.availability_zone,
        "imageId": "ami-00000000",
        "instanceId": meta.instance_id,
        "instanceType": "baremetal",
        "privateIp": meta.local_ipv4,
        "region": meta.availability_zone,
        "version": "2017-09-30"
    });

    Ok((
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string_pretty(&doc).unwrap_or_default(),
    ))
}

// --- Core metadata endpoints ---

pub async fn ami_id() -> &'static str {
    "ami-00000000"
}

pub async fn instance_id(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    let meta = lookup_or_404(&state, &ip)?;
    Ok(meta.instance_id)
}

pub async fn instance_type() -> &'static str {
    "baremetal"
}

pub async fn hostname(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    let meta = lookup_or_404(&state, &ip)?;
    Ok(meta.hostname)
}

pub async fn local_hostname(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    let meta = lookup_or_404(&state, &ip)?;
    Ok(meta.local_hostname)
}

pub async fn local_ipv4(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    let meta = lookup_or_404(&state, &ip)?;
    Ok(meta.local_ipv4)
}

pub async fn mac(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    // Return a synthetic MAC based on IP for consistency
    let meta = lookup_or_404(&state, &ip)?;
    // Look up BMH to get real MAC if available
    let bmh = state.get_bmh(&meta.local_hostname).await;
    let mac_addr = bmh
        .as_ref()
        .map(|b| b.spec.boot_mac_address.clone())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| format!("02:00:{}", meta.local_ipv4.replace('.', ":")));
    Ok(mac_addr)
}

pub async fn availability_zone(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    let meta = lookup_or_404(&state, &ip)?;
    Ok(meta.availability_zone.clone())
}

pub async fn region(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    let meta = lookup_or_404(&state, &ip)?;
    Ok(meta.availability_zone.clone())
}

pub async fn network_interfaces_macs(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    let meta = lookup_or_404(&state, &ip)?;
    let bmh = state.get_bmh(&meta.local_hostname).await;
    let mac_addr = bmh
        .as_ref()
        .map(|b| b.spec.boot_mac_address.clone())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| format!("02:00:{}", meta.local_ipv4.replace('.', ":")));
    Ok(format!("{}/\n", mac_addr))
}

pub async fn network_mac_detail(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(_mac): Path<String>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    let _meta = lookup_or_404(&state, &ip)?;
    Ok("device-number\nlocal-ipv4s\nsubnet-ipv4-cidr-block\n".to_string())
}

pub async fn network_mac_device_number(
    Path(_mac): Path<String>,
) -> &'static str {
    "0"
}

pub async fn network_mac_local_ipv4s(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(_mac): Path<String>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    let meta = lookup_or_404(&state, &ip)?;
    Ok(meta.local_ipv4)
}

pub async fn network_mac_subnet(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(_mac): Path<String>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    let meta = lookup_or_404(&state, &ip)?;
    // Derive /24 from the IP
    let parts: Vec<&str> = meta.local_ipv4.split('.').collect();
    if parts.len() == 4 {
        Ok(format!("{}.{}.{}.0/24", parts[0], parts[1], parts[2]))
    } else {
        Ok(meta.local_ipv4)
    }
}

pub async fn public_keys_index(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    let meta = lookup_or_404(&state, &ip)?;

    let listing: String = meta
        .public_keys
        .iter()
        .enumerate()
        .map(|(i, pk)| format!("{}={}\n", i, pk.ssh_user))
        .collect();

    Ok(listing)
}

pub async fn public_key(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(index): Path<usize>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    let meta = lookup_or_404(&state, &ip)?;

    let entry = meta
        .public_keys
        .get(index)
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(entry.keys.join("\n"))
}

pub async fn user_data(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    let meta = lookup_or_404(&state, &ip)?;
    Ok(meta.user_data)
}

/// Serve Ignition v3 config for the requesting host.
pub async fn ignition_config(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Result<(StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String), StatusCode> {
    let ip = get_source_ip(&addr);

    let meta = match state.resolve_on_demand(&ip).await {
        Some(m) => m,
        None => {
            warn!(%ip, "ignition request from unknown IP");
            return Err(StatusCode::NOT_FOUND);
        }
    };

    let bmh = state.get_bmh(&meta.local_hostname).await;
    let config = provision::build_ignition(&meta, bmh.as_ref());

    info!(%ip, host = %meta.local_hostname, "serving ignition config");
    Ok((
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        config,
    ))
}

/// Serve kickstart config for the requesting host.
pub async fn kickstart_config(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Result<(StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String), StatusCode> {
    let ip = get_source_ip(&addr);

    let meta = match state.resolve_on_demand(&ip).await {
        Some(m) => m,
        None => {
            warn!(%ip, "kickstart request from unknown IP");
            return Err(StatusCode::NOT_FOUND);
        }
    };

    let bmh = state.get_bmh(&meta.local_hostname).await;
    let config = provision::build_kickstart(&meta, bmh.as_ref());

    info!(%ip, host = %meta.local_hostname, "serving kickstart config");
    Ok((
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/plain")],
        config,
    ))
}

pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

fn lookup_or_404(
    state: &AppState,
    ip: &std::net::IpAddr,
) -> Result<crate::model::HostMetadata, StatusCode> {
    match state.get_metadata(ip) {
        Some(meta) => {
            debug!(%ip, host = %meta.instance_id, "metadata served");
            Ok(meta)
        }
        None => {
            warn!(%ip, "metadata request from unknown IP");
            Err(StatusCode::NOT_FOUND)
        }
    }
}
