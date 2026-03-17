mod handlers;

use crate::cache::AppState;
use axum::extract::ConnectInfo;
use axum::middleware;
use axum::routing::{get, put};
use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;

async fn access_log(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: axum::http::Request<axum::body::Body>,
    next: middleware::Next,
) -> axum::response::Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let ip = addr.ip();
    let resp = next.run(req).await;
    let status = resp.status().as_u16();
    // Skip health probes to reduce noise
    if uri.path() != "/health" {
        tracing::info!(%ip, %method, path = %uri, status, "request");
    }
    resp
}

/// Metadata routes that need shared state.
fn metadata_routes() -> Router<Arc<AppState>> {
    Router::new()
        // IMDSv2 token
        .route("/api/token", put(handlers::api_token))
        // Directory listings
        .route("/", get(handlers::latest))
        .route("/dynamic", get(handlers::dynamic_index))
        .route("/dynamic/", get(handlers::dynamic_index))
        .route(
            "/dynamic/instance-identity",
            get(handlers::instance_identity_index),
        )
        .route(
            "/dynamic/instance-identity/",
            get(handlers::instance_identity_index),
        )
        .route(
            "/dynamic/instance-identity/document",
            get(handlers::instance_identity_document),
        )
        // meta-data index
        .route("/meta-data", get(handlers::meta_data_index))
        .route("/meta-data/", get(handlers::meta_data_index))
        // Core metadata
        .route("/meta-data/ami-id", get(handlers::ami_id))
        .route("/meta-data/instance-id", get(handlers::instance_id))
        .route("/meta-data/instance-type", get(handlers::instance_type))
        .route("/meta-data/hostname", get(handlers::hostname))
        .route("/meta-data/local-hostname", get(handlers::local_hostname))
        .route("/meta-data/local-ipv4", get(handlers::local_ipv4))
        .route("/meta-data/mac", get(handlers::mac))
        // Placement
        .route("/meta-data/placement", get(handlers::placement_index))
        .route("/meta-data/placement/", get(handlers::placement_index))
        .route(
            "/meta-data/placement/availability-zone",
            get(handlers::availability_zone),
        )
        .route("/meta-data/placement/region", get(handlers::region))
        // Services
        .route("/meta-data/services", get(handlers::services_index))
        .route("/meta-data/services/", get(handlers::services_index))
        .route(
            "/meta-data/services/domain",
            get(handlers::services_domain),
        )
        .route(
            "/meta-data/services/partition",
            get(handlers::services_partition),
        )
        // Network interfaces
        .route("/meta-data/network", get(handlers::network_index))
        .route("/meta-data/network/", get(handlers::network_index))
        .route(
            "/meta-data/network/interfaces",
            get(handlers::network_interfaces_index),
        )
        .route(
            "/meta-data/network/interfaces/",
            get(handlers::network_interfaces_index),
        )
        .route(
            "/meta-data/network/interfaces/macs",
            get(handlers::network_interfaces_macs),
        )
        .route(
            "/meta-data/network/interfaces/macs/",
            get(handlers::network_interfaces_macs),
        )
        .route(
            "/meta-data/network/interfaces/macs/{mac}/",
            get(handlers::network_mac_detail),
        )
        .route(
            "/meta-data/network/interfaces/macs/{mac}/device-number",
            get(handlers::network_mac_device_number),
        )
        .route(
            "/meta-data/network/interfaces/macs/{mac}/local-ipv4s",
            get(handlers::network_mac_local_ipv4s),
        )
        .route(
            "/meta-data/network/interfaces/macs/{mac}/subnet-ipv4-cidr-block",
            get(handlers::network_mac_subnet),
        )
        // Public keys
        .route("/meta-data/public-keys", get(handlers::public_keys_index))
        .route(
            "/meta-data/public-keys/",
            get(handlers::public_keys_index),
        )
        .route(
            "/meta-data/public-keys/{index}/openssh-key",
            get(handlers::public_key),
        )
        // User data
        .route("/user-data", get(handlers::user_data))
}

/// Build the EC2-compatible metadata router with provisioning endpoints.
pub fn router(state: Arc<AppState>) -> Router {
    let versioned = metadata_routes();

    Router::new()
        .route("/", get(handlers::root))
        // Mount metadata routes under /latest
        .nest("/latest", versioned.clone())
        // Mount same routes under versioned prefixes (cloud-init uses these)
        .nest("/2021-01-03", versioned.clone())
        .nest("/2020-10-27", versioned.clone())
        .nest("/2019-10-01", versioned.clone())
        .nest("/2018-09-24", versioned.clone())
        .nest("/2016-09-02", versioned.clone())
        .nest("/2009-04-04", versioned)
        // Provisioning endpoints
        .route("/config/ignition", get(handlers::ignition_config))
        .route("/config/kickstart", get(handlers::kickstart_config))
        .route("/health", get(handlers::health))
        .layer(middleware::from_fn(access_log))
        .with_state(state)
}
