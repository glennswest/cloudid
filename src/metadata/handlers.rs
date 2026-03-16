use crate::cache::AppState;
use crate::provision;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{debug, info, warn};

type AppResponse = Result<String, StatusCode>;

fn get_source_ip(addr: &SocketAddr) -> std::net::IpAddr {
    addr.ip()
}

pub async fn root() -> &'static str {
    "latest\n"
}

pub async fn latest() -> &'static str {
    "meta-data\nuser-data\n"
}

pub async fn meta_data_index() -> &'static str {
    "instance-id\nhostname\nlocal-hostname\nlocal-ipv4\nplacement/\npublic-keys/\n"
}

pub async fn placement_index() -> &'static str {
    "availability-zone\n"
}

pub async fn instance_id(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    let meta = lookup_or_404(&state, &ip)?;
    Ok(meta.instance_id)
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

pub async fn availability_zone(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResponse {
    let ip = get_source_ip(&addr);
    let meta = lookup_or_404(&state, &ip)?;
    Ok(meta.availability_zone)
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
///
/// If the BMH has a base ignition config in `spec.ignition`, SSH keys from
/// the identity pipeline are merged into it. Otherwise a default config is generated.
pub async fn ignition_config(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Result<(StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String), StatusCode> {
    let ip = get_source_ip(&addr);

    // Use on-demand resolve for provisioning (more robust during initial boot)
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
///
/// If the BMH has a base kickstart config in `spec.kickstart`, SSH keys from
/// the identity pipeline are merged into it. Otherwise a default config is generated.
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
