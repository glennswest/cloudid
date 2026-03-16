mod handlers;

use crate::cache::AppState;
use axum::routing::get;
use axum::Router;
use std::sync::Arc;

/// Build the EC2-compatible metadata router with provisioning endpoints.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(handlers::root))
        .route("/latest", get(handlers::latest))
        .route("/latest/", get(handlers::latest))
        .route("/latest/meta-data", get(handlers::meta_data_index))
        .route("/latest/meta-data/", get(handlers::meta_data_index))
        .route(
            "/latest/meta-data/instance-id",
            get(handlers::instance_id),
        )
        .route("/latest/meta-data/hostname", get(handlers::hostname))
        .route(
            "/latest/meta-data/local-hostname",
            get(handlers::local_hostname),
        )
        .route(
            "/latest/meta-data/local-ipv4",
            get(handlers::local_ipv4),
        )
        .route(
            "/latest/meta-data/placement",
            get(handlers::placement_index),
        )
        .route(
            "/latest/meta-data/placement/",
            get(handlers::placement_index),
        )
        .route(
            "/latest/meta-data/placement/availability-zone",
            get(handlers::availability_zone),
        )
        .route(
            "/latest/meta-data/public-keys",
            get(handlers::public_keys_index),
        )
        .route(
            "/latest/meta-data/public-keys/",
            get(handlers::public_keys_index),
        )
        .route(
            "/latest/meta-data/public-keys/{index}/openssh-key",
            get(handlers::public_key),
        )
        .route("/latest/user-data", get(handlers::user_data))
        // Provisioning endpoints
        .route("/config/ignition", get(handlers::ignition_config))
        .route("/config/kickstart", get(handlers::kickstart_config))
        .route("/health", get(handlers::health))
        .with_state(state)
}
