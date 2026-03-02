use crate::cache::AppState;
use crate::model::{GroupResource, HostAccessResource, HostGroupResource, UserResource};
use anyhow::Result;
use futures::StreamExt;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

const BUCKET_USERS: &str = "AMO_USERS";
const BUCKET_GROUPS: &str = "AMO_GROUPS";
const BUCKET_HOSTACCESS: &str = "AMO_HOSTACCESS";
const BUCKET_HOSTGROUPS: &str = "AMO_HOSTGROUPS";

/// Start the AMO NATS watcher. Connects to NATS, watches all four KV buckets,
/// and updates the identity state in the AppState cache.
///
/// Self-healing: on disconnect, serves from cache and reconnects with exponential backoff.
pub async fn start(state: Arc<AppState>) {
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);

    loop {
        info!(url = %state.config.amo.nats_url, "connecting to AMO NATS");

        match run_watcher(&state).await {
            Ok(()) => {
                info!("AMO NATS watcher exited cleanly");
                backoff = Duration::from_secs(1);
            }
            Err(e) => {
                error!(error = %e, "AMO NATS watcher failed");
            }
        }

        warn!(backoff_secs = backoff.as_secs(), "reconnecting to AMO NATS");
        sleep(backoff).await;
        backoff = (backoff * 2).min(max_backoff);
    }
}

async fn run_watcher(state: &Arc<AppState>) -> Result<()> {
    let client = async_nats::connect(&state.config.amo.nats_url).await?;
    let jetstream = async_nats::jetstream::new(client);

    info!("connected to AMO NATS, loading initial state");

    // Load initial state from all buckets
    load_bucket_users(&jetstream, state).await?;
    load_bucket_groups(&jetstream, state).await?;
    load_bucket_hostaccess(&jetstream, state).await?;
    load_bucket_hostgroups(&jetstream, state).await?;

    // Rebuild cache after initial load
    state.rebuild_cache().await;
    info!("initial AMO state loaded, starting watches");

    // Watch all buckets concurrently
    let s1 = state.clone();
    let s2 = state.clone();
    let s3 = state.clone();
    let s4 = state.clone();
    let js1 = jetstream.clone();
    let js2 = jetstream.clone();
    let js3 = jetstream.clone();
    let js4 = jetstream.clone();

    tokio::select! {
        r = watch_users(js1, s1) => r,
        r = watch_groups(js2, s2) => r,
        r = watch_hostaccess(js3, s3) => r,
        r = watch_hostgroups(js4, s4) => r,
    }
}

async fn load_bucket_users(
    js: &async_nats::jetstream::Context,
    state: &Arc<AppState>,
) -> Result<()> {
    let store = js.get_key_value(BUCKET_USERS).await?;
    let mut keys = store.keys().await?;
    let mut identity = state.identity.write().await;

    while let Some(key) = keys.next().await {
        let key = key?;
        if let Some(entry) = store.get(&key).await? {
            match serde_json::from_slice::<UserResource>(&entry) {
                Ok(user) => {
                    identity.users.insert(user.metadata.name.clone(), user);
                }
                Err(e) => {
                    warn!(key, error = %e, "failed to parse user from NATS KV");
                }
            }
        }
    }

    info!(count = identity.users.len(), "loaded users from AMO");
    Ok(())
}

async fn load_bucket_groups(
    js: &async_nats::jetstream::Context,
    state: &Arc<AppState>,
) -> Result<()> {
    let store = js.get_key_value(BUCKET_GROUPS).await?;
    let mut keys = store.keys().await?;
    let mut identity = state.identity.write().await;

    while let Some(key) = keys.next().await {
        let key = key?;
        if let Some(entry) = store.get(&key).await? {
            match serde_json::from_slice::<GroupResource>(&entry) {
                Ok(group) => {
                    identity.groups.insert(group.metadata.name.clone(), group);
                }
                Err(e) => {
                    warn!(key, error = %e, "failed to parse group from NATS KV");
                }
            }
        }
    }

    info!(count = identity.groups.len(), "loaded groups from AMO");
    Ok(())
}

async fn load_bucket_hostaccess(
    js: &async_nats::jetstream::Context,
    state: &Arc<AppState>,
) -> Result<()> {
    let store = js.get_key_value(BUCKET_HOSTACCESS).await?;
    let mut keys = store.keys().await?;
    let mut identity = state.identity.write().await;

    while let Some(key) = keys.next().await {
        let key = key?;
        if let Some(entry) = store.get(&key).await? {
            match serde_json::from_slice::<HostAccessResource>(&entry) {
                Ok(ha) => {
                    identity.host_access.insert(ha.metadata.name.clone(), ha);
                }
                Err(e) => {
                    warn!(key, error = %e, "failed to parse hostaccess from NATS KV");
                }
            }
        }
    }

    info!(count = identity.host_access.len(), "loaded hostaccess from AMO");
    Ok(())
}

async fn load_bucket_hostgroups(
    js: &async_nats::jetstream::Context,
    state: &Arc<AppState>,
) -> Result<()> {
    let store = js.get_key_value(BUCKET_HOSTGROUPS).await?;
    let mut keys = store.keys().await?;
    let mut identity = state.identity.write().await;

    while let Some(key) = keys.next().await {
        let key = key?;
        if let Some(entry) = store.get(&key).await? {
            match serde_json::from_slice::<HostGroupResource>(&entry) {
                Ok(hg) => {
                    identity
                        .host_groups
                        .insert(hg.metadata.name.clone(), hg);
                }
                Err(e) => {
                    warn!(key, error = %e, "failed to parse hostgroup from NATS KV");
                }
            }
        }
    }

    info!(
        count = identity.host_groups.len(),
        "loaded hostgroups from AMO"
    );
    Ok(())
}

async fn watch_users(
    js: async_nats::jetstream::Context,
    state: Arc<AppState>,
) -> Result<()> {
    let store = js.get_key_value(BUCKET_USERS).await?;
    let mut watcher = store.watch_all().await?;

    while let Some(entry) = watcher.next().await {
        let entry = entry?;
        match entry.operation {
            async_nats::jetstream::kv::Operation::Put => {
                match serde_json::from_slice::<UserResource>(&entry.value) {
                    Ok(user) => {
                        info!(user = %user.metadata.name, "user updated via NATS");
                        let mut identity = state.identity.write().await;
                        identity.users.insert(user.metadata.name.clone(), user);
                        drop(identity);
                        state.rebuild_cache().await;
                    }
                    Err(e) => {
                        warn!(key = %entry.key, error = %e, "failed to parse user update");
                    }
                }
            }
            async_nats::jetstream::kv::Operation::Delete
            | async_nats::jetstream::kv::Operation::Purge => {
                info!(key = %entry.key, "user deleted via NATS");
                let mut identity = state.identity.write().await;
                identity.users.remove(&entry.key);
                drop(identity);
                state.rebuild_cache().await;
            }
        }
    }

    Ok(())
}

async fn watch_groups(
    js: async_nats::jetstream::Context,
    state: Arc<AppState>,
) -> Result<()> {
    let store = js.get_key_value(BUCKET_GROUPS).await?;
    let mut watcher = store.watch_all().await?;

    while let Some(entry) = watcher.next().await {
        let entry = entry?;
        match entry.operation {
            async_nats::jetstream::kv::Operation::Put => {
                match serde_json::from_slice::<GroupResource>(&entry.value) {
                    Ok(group) => {
                        info!(group = %group.metadata.name, "group updated via NATS");
                        let mut identity = state.identity.write().await;
                        identity.groups.insert(group.metadata.name.clone(), group);
                        drop(identity);
                        state.rebuild_cache().await;
                    }
                    Err(e) => {
                        warn!(key = %entry.key, error = %e, "failed to parse group update");
                    }
                }
            }
            async_nats::jetstream::kv::Operation::Delete
            | async_nats::jetstream::kv::Operation::Purge => {
                info!(key = %entry.key, "group deleted via NATS");
                let mut identity = state.identity.write().await;
                identity.groups.remove(&entry.key);
                drop(identity);
                state.rebuild_cache().await;
            }
        }
    }

    Ok(())
}

async fn watch_hostaccess(
    js: async_nats::jetstream::Context,
    state: Arc<AppState>,
) -> Result<()> {
    let store = js.get_key_value(BUCKET_HOSTACCESS).await?;
    let mut watcher = store.watch_all().await?;

    while let Some(entry) = watcher.next().await {
        let entry = entry?;
        match entry.operation {
            async_nats::jetstream::kv::Operation::Put => {
                match serde_json::from_slice::<HostAccessResource>(&entry.value) {
                    Ok(ha) => {
                        info!(rule = %ha.metadata.name, "hostaccess updated via NATS");
                        let mut identity = state.identity.write().await;
                        identity.host_access.insert(ha.metadata.name.clone(), ha);
                        drop(identity);
                        state.rebuild_cache().await;
                    }
                    Err(e) => {
                        warn!(key = %entry.key, error = %e, "failed to parse hostaccess update");
                    }
                }
            }
            async_nats::jetstream::kv::Operation::Delete
            | async_nats::jetstream::kv::Operation::Purge => {
                info!(key = %entry.key, "hostaccess deleted via NATS");
                let mut identity = state.identity.write().await;
                identity.host_access.remove(&entry.key);
                drop(identity);
                state.rebuild_cache().await;
            }
        }
    }

    Ok(())
}

async fn watch_hostgroups(
    js: async_nats::jetstream::Context,
    state: Arc<AppState>,
) -> Result<()> {
    let store = js.get_key_value(BUCKET_HOSTGROUPS).await?;
    let mut watcher = store.watch_all().await?;

    while let Some(entry) = watcher.next().await {
        let entry = entry?;
        match entry.operation {
            async_nats::jetstream::kv::Operation::Put => {
                match serde_json::from_slice::<HostGroupResource>(&entry.value) {
                    Ok(hg) => {
                        info!(group = %hg.metadata.name, "hostgroup updated via NATS");
                        let mut identity = state.identity.write().await;
                        identity
                            .host_groups
                            .insert(hg.metadata.name.clone(), hg);
                        drop(identity);
                        state.rebuild_cache().await;
                    }
                    Err(e) => {
                        warn!(key = %entry.key, error = %e, "failed to parse hostgroup update");
                    }
                }
            }
            async_nats::jetstream::kv::Operation::Delete
            | async_nats::jetstream::kv::Operation::Purge => {
                info!(key = %entry.key, "hostgroup deleted via NATS");
                let mut identity = state.identity.write().await;
                identity.host_groups.remove(&entry.key);
                drop(identity);
                state.rebuild_cache().await;
            }
        }
    }

    Ok(())
}
