use crate::cache::AppState;
use crate::provision;
use crate::templates::{
    self, Assignment, AssignmentRequest, TemplateBundle, TemplateCreateRequest,
    TemplateListResponse,
};
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
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
) -> Result<(StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String), StatusCode> {
    let ip = get_source_ip(&addr);

    let meta = match state.resolve_on_demand(&ip).await {
        Some(m) => m,
        None => {
            warn!(%ip, "user-data request from unknown IP");
            return Err(StatusCode::NOT_FOUND);
        }
    };

    let bmh = state.get_bmh(&meta.local_hostname).await;

    // Try template system first
    match provision::resolve_and_build(&state, &meta.local_hostname, &meta, bmh.as_ref()).await {
        provision::TemplateResult::Config { content, format } => {
            info!(%ip, host = %meta.local_hostname, "serving user-data (template)");
            Ok((
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, format.as_content_type())],
                content,
            ))
        }
        provision::TemplateResult::None => {
            // Fall back to default ignition generation
            let config = provision::build_ignition(&meta, bmh.as_ref());
            info!(%ip, host = %meta.local_hostname, "serving user-data (ignition)");
            Ok((
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                config,
            ))
        }
    }
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

    // Try template system first
    match provision::resolve_and_build(&state, &meta.local_hostname, &meta, bmh.as_ref()).await {
        provision::TemplateResult::Config { content, format } => {
            info!(%ip, host = %meta.local_hostname, "serving ignition config (template)");
            Ok((
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, format.as_content_type())],
                content,
            ))
        }
        provision::TemplateResult::None => {
            let config = provision::build_ignition(&meta, bmh.as_ref());
            info!(%ip, host = %meta.local_hostname, "serving ignition config");
            Ok((
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                config,
            ))
        }
    }
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

    // Try template system first
    match provision::resolve_and_build(&state, &meta.local_hostname, &meta, bmh.as_ref()).await {
        provision::TemplateResult::Config { content, format } => {
            info!(%ip, host = %meta.local_hostname, "serving kickstart config (template)");
            Ok((
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, format.as_content_type())],
                content,
            ))
        }
        provision::TemplateResult::None => {
            let config = provision::build_kickstart(&meta, bmh.as_ref());
            info!(%ip, host = %meta.local_hostname, "serving kickstart config");
            Ok((
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "text/plain")],
                config,
            ))
        }
    }
}

pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

/// GET /api/v1/debug/state — dump container watcher state, BMH state, and cache stats.
pub async fn debug_state(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let containers = state.containers.read().await;
    let bmh = state.bmh.read().await;
    let identity = state.identity.read().await;

    let container_ips: Vec<serde_json::Value> = containers
        .ip_to_container
        .iter()
        .map(|(ip, info)| {
            serde_json::json!({
                "ip": ip.to_string(),
                "namespace": info.namespace,
                "pod": info.pod_name,
                "container": info.container_name,
                "hostname": info.hostname,
            })
        })
        .collect();

    let ns_owners: serde_json::Map<String, serde_json::Value> = containers
        .namespace_owners
        .iter()
        .map(|(ns, owner)| (ns.clone(), serde_json::Value::String(owner.clone())))
        .collect();

    let bmh_ips: Vec<serde_json::Value> = bmh
        .ip_to_hostname
        .iter()
        .map(|(ip, hostname)| {
            serde_json::json!({ "ip": ip.to_string(), "hostname": hostname })
        })
        .collect();

    let cache_entries: Vec<serde_json::Value> = state
        .metadata_cache
        .iter()
        .map(|entry| {
            let ip = entry.key();
            let meta = entry.value();
            serde_json::json!({
                "ip": ip.to_string(),
                "instance_id": meta.instance_id,
                "hostname": meta.hostname,
                "public_keys": meta.public_keys.iter().map(|pk| {
                    serde_json::json!({
                        "ssh_user": pk.ssh_user,
                        "key_count": pk.keys.len(),
                    })
                }).collect::<Vec<_>>(),
            })
        })
        .collect();

    Json(serde_json::json!({
        "container_ips": container_ips,
        "container_ip_count": container_ips.len(),
        "namespace_owners": ns_owners,
        "bmh_ips": bmh_ips,
        "bmh_ip_count": bmh_ips.len(),
        "identity_users": identity.users.keys().collect::<Vec<_>>(),
        "identity_host_access_rules": identity.host_access.keys().collect::<Vec<_>>(),
        "cache_entries": cache_entries,
        "cache_entry_count": cache_entries.len(),
    }))
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

// --- Template CRUD handlers ---

/// GET /api/v1/templates — list all templates.
pub async fn templates_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<TemplateListResponse>, StatusCode> {
    let templates = state
        .template_store
        .list_all()
        .await
        .map_err(|e| {
            warn!(error = %e, "failed to list templates");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(TemplateListResponse { templates }))
}

/// GET /api/v1/templates/:image_type — list templates for an image type.
pub async fn templates_list_by_type(
    State(state): State<Arc<AppState>>,
    Path(image_type): Path<String>,
) -> Result<Json<TemplateListResponse>, StatusCode> {
    let templates = state
        .template_store
        .list_by_type(&image_type)
        .await
        .map_err(|e| {
            warn!(error = %e, image_type, "failed to list templates by type");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(TemplateListResponse { templates }))
}

/// GET /api/v1/templates/:image_type/:name — get a single template.
pub async fn templates_get(
    State(state): State<Arc<AppState>>,
    Path((image_type, name)): Path<(String, String)>,
) -> Result<Json<templates::LoadedTemplate>, StatusCode> {
    match state.template_store.get(&image_type, &name).await {
        Ok(Some(tpl)) => Ok(Json(tpl)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            warn!(error = %e, image_type, name, "failed to get template");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// PUT /api/v1/templates/:image_type/:name — create or update a template.
pub async fn templates_put(
    State(state): State<Arc<AppState>>,
    Path((image_type, name)): Path<(String, String)>,
    Json(req): Json<TemplateCreateRequest>,
) -> Result<(StatusCode, Json<templates::LoadedTemplate>), StatusCode> {
    let tpl = state
        .template_store
        .put(&image_type, &name, &req)
        .await
        .map_err(|e| {
            warn!(error = %e, image_type, name, "failed to put template");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok((StatusCode::OK, Json(tpl)))
}

/// DELETE /api/v1/templates/:image_type/:name — delete a template.
pub async fn templates_delete(
    State(state): State<Arc<AppState>>,
    Path((image_type, name)): Path<(String, String)>,
) -> StatusCode {
    match state.template_store.delete(&image_type, &name).await {
        Ok(true) => StatusCode::NO_CONTENT,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(e) => {
            warn!(error = %e, image_type, name, "failed to delete template");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

// --- Backup/Restore handlers ---

/// GET /api/v1/templates/backup — export all templates as a JSON bundle.
pub async fn templates_backup(
    State(state): State<Arc<AppState>>,
) -> Result<Json<TemplateBundle>, StatusCode> {
    let bundle = state
        .template_store
        .backup()
        .await
        .map_err(|e| {
            warn!(error = %e, "failed to backup templates");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(bundle))
}

/// POST /api/v1/templates/restore — import templates from a JSON bundle.
pub async fn templates_restore(
    State(state): State<Arc<AppState>>,
    Json(bundle): Json<TemplateBundle>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let count = state
        .template_store
        .restore(&bundle)
        .await
        .map_err(|e| {
            warn!(error = %e, "failed to restore templates");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(serde_json::json!({ "restored": count })))
}

// --- Assignment handlers ---

/// GET /api/v1/assignments — list all host-to-template assignments.
pub async fn assignments_list(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let assignments = state.assignments.read().await;
    Json(serde_json::to_value(&*assignments).unwrap_or_default())
}

/// PUT /api/v1/assignments/:hostname — assign a template to a host.
pub async fn assignments_put(
    State(state): State<Arc<AppState>>,
    Path(hostname): Path<String>,
    Json(req): Json<AssignmentRequest>,
) -> StatusCode {
    let mut assignments = state.assignments.write().await;
    assignments.assignments.insert(
        hostname.clone(),
        Assignment {
            image_type: req.image_type,
            template: req.template,
        },
    );
    if let Err(e) = state.template_store.save_assignments(&assignments).await {
        warn!(error = %e, "failed to persist assignments");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    info!(hostname, "template assignment updated");
    StatusCode::OK
}

/// DELETE /api/v1/assignments/:hostname — remove a template assignment.
pub async fn assignments_delete(
    State(state): State<Arc<AppState>>,
    Path(hostname): Path<String>,
) -> StatusCode {
    let mut assignments = state.assignments.write().await;
    if assignments.assignments.remove(&hostname).is_none() {
        return StatusCode::NOT_FOUND;
    }
    if let Err(e) = state.template_store.save_assignments(&assignments).await {
        warn!(error = %e, "failed to persist assignments");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    info!(hostname, "template assignment removed");
    StatusCode::NO_CONTENT
}

// --- Oneshot handlers ---

/// POST /config/provisioned — host marks its oneshot template as complete (by source IP).
pub async fn oneshot_provisioned(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> StatusCode {
    let ip = get_source_ip(&addr);

    let hostname = {
        let bmh = state.bmh.read().await;
        match bmh.ip_to_hostname.get(&ip) {
            Some(h) => h.clone(),
            None => {
                warn!(%ip, "provisioned request from unknown IP");
                return StatusCode::NOT_FOUND;
            }
        }
    };

    let mut oneshot = state.oneshot.write().await;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    oneshot.completed.insert(hostname.clone(), ts);

    if let Err(e) = state.template_store.save_oneshot(&oneshot).await {
        warn!(error = %e, "failed to persist oneshot state");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    info!(%ip, hostname, "oneshot provisioning marked complete");
    StatusCode::OK
}

/// GET /api/v1/oneshot — list all oneshot completion states.
pub async fn oneshot_list(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let oneshot = state.oneshot.read().await;
    Json(serde_json::to_value(&*oneshot).unwrap_or_default())
}

/// DELETE /api/v1/oneshot/:hostname — reset oneshot for re-provisioning.
pub async fn oneshot_delete(
    State(state): State<Arc<AppState>>,
    Path(hostname): Path<String>,
) -> StatusCode {
    let mut oneshot = state.oneshot.write().await;
    if oneshot.completed.remove(&hostname).is_none() {
        return StatusCode::NOT_FOUND;
    }
    if let Err(e) = state.template_store.save_oneshot(&oneshot).await {
        warn!(error = %e, "failed to persist oneshot state");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    info!(hostname, "oneshot state reset for re-provisioning");
    StatusCode::NO_CONTENT
}

// --- Diagnostics ---

/// GET /config/template — returns template info for the requesting host (by source IP).
pub async fn template_info(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let ip = get_source_ip(&addr);

    let hostname = {
        let bmh = state.bmh.read().await;
        bmh.ip_to_hostname.get(&ip).cloned()
    };

    let hostname = match hostname {
        Some(h) => h,
        None => {
            return Ok(Json(serde_json::json!({
                "ip": ip.to_string(),
                "hostname": null,
                "template": null,
                "reason": "unknown IP"
            })));
        }
    };

    // Check oneshot
    let oneshot_done = {
        let oneshot = state.oneshot.read().await;
        oneshot.completed.contains_key(&hostname)
    };

    if oneshot_done {
        return Ok(Json(serde_json::json!({
            "ip": ip.to_string(),
            "hostname": hostname,
            "template": null,
            "reason": "oneshot completed"
        })));
    }

    // Check assignments
    let bmh = state.get_bmh(&hostname).await;
    let bmh_assignment = bmh.as_ref().and_then(|b| {
        b.spec
            .template
            .as_ref()
            .filter(|t| !t.is_empty())
            .map(|tpl| ("bmh".to_string(), tpl.clone()))
    });

    // Try REST assignment (sync read via try_read)
    let rest_assignment = {
        let assignments = state.assignments.try_read();
        match assignments {
            Ok(a) => a.assignments.get(&hostname).map(|asgn| {
                (
                    "rest_assignment".to_string(),
                    format!("{}/{}", asgn.image_type, asgn.template),
                )
            }),
            Err(_) => None,
        }
    };

    let config_assignment = state
        .config
        .templates
        .assignments
        .iter()
        .find(|r| r.hosts.iter().any(|h| h == &hostname || h == "*"))
        .map(|r| ("config".to_string(), r.template.clone()));

    let (source, template_ref) = bmh_assignment
        .or(rest_assignment)
        .or(config_assignment)
        .unzip();

    Ok(Json(serde_json::json!({
        "ip": ip.to_string(),
        "hostname": hostname,
        "template": template_ref,
        "source": source,
        "oneshot_completed": false
    })))
}
