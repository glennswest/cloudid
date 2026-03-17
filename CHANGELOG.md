# Changelog

## [Unreleased]

### 2026-03-17
- **fix:** user-data endpoint returns Ignition JSON instead of cloud-config (FCOS rejects `#cloud-config` prefix as invalid JSON)
- **feat:** EC2 IMDSv2 support — PUT /latest/api/token with per-host token generation and storage
- **feat:** Instance identity document — GET /latest/dynamic/instance-identity/document (JSON)
- **feat:** Versioned API paths — /2009-04-04/ through /2021-01-03/ (same as /latest/)
- **feat:** Additional metadata endpoints: ami-id, instance-type, mac, region, services/, network/interfaces/macs/
- **feat:** Custom access log middleware with source IP, method, path, status (health probes excluded)
- **fix:** DHCP pool listing deserialization — MicroDNS returns bare JSON array, not `{ pools: [...] }` wrapper
- **fix:** TraceLayer configured at INFO level (was defaulting to DEBUG)

### 2026-03-16
- **feat:** Static identity config -- define users and SSH keys from .pub files (works without AMO NATS)
- **feat:** Wildcard host matching -- `hosts = ["*"]` matches all BMH hosts
- **feat:** Ignition v3.4.0 config generation and serving (`/config/ignition`)
- **feat:** Kickstart config generation and serving (`/config/kickstart`)
- **feat:** BMH-sourced provisioning -- ignition/kickstart base configs from BMH CRD `spec.ignition` and `spec.kickstart`
- **feat:** SSH key merging -- identity pipeline keys merged into BMH-provided base configs
- **feat:** On-demand resolve for provisioning endpoints (handles race conditions during PXE boot)
- **feat:** Full BMH data storage in cache (for provisioning config generation)
- **feat:** Auto-discover data networks from mkube and configure DHCP option 121 metadata route via MicroDNS API
- **feat:** MikroTik DNAT rule: 169.254.169.254:80 -> 192.168.200.20:8090
- **feat:** Static IP 192.168.200.20 for cloudid on gt network
- **refactor:** SSH keys from .pub files instead of inline TOML
- **docs:** MicroDNS DHCP option 121 API spec (docs/microdns-dhcp-option121-spec.md)
- **test:** Added tests for wildcard matching, ignition generation/merge, kickstart generation/merge (8 total)
- **build:** build.sh and deploy.sh scripts (matching microdns pattern)
- **build:** .cargo/config.toml for ARM64 musl cross-compilation
- **build:** Dockerfile simplified (config from ConfigMap, not baked in)
- **refactor:** BMH namespaces auto-discovered from mkube `/api/v1/networks` (type=data), removed `bmh_namespaces` config
- **fix:** Changed gt network type from `data` to `management` in mkube (no BMH hosts on gt)

### 2026-03-01
- **feat:** Initial Rust project scaffolding (Cargo.toml, config, model types)
- **feat:** axum server skeleton with health check endpoint
- **feat:** TOML config parsing (server, AMO, mkube, metadata settings)
- **feat:** Core model types (User, Group, HostAccess, HostGroup)
- **feat:** Tracing/logging setup with env-filter
- **feat:** Metadata cache (IP -> precomputed host metadata)
- **feat:** Resolution pipeline (IP -> hostname -> HostAccess -> users -> SSH keys)
- **feat:** AMO NATS JetStream KV watcher (users, groups, hostaccess, hostgroups)
- **feat:** mkube BMH HTTP watcher with watch reconnection
- **feat:** EC2-compatible metadata endpoint (/latest/meta-data/*, /latest/user-data)
- **feat:** cloudid-agent binary (refresh, authorized-keys, status subcommands)
- **feat:** Dockerfile (FROM scratch, static musl binary)
- **feat:** Example config.toml
- **feat:** mkube pod manifest and ConfigMap for deployment (deploy/)
