# Changelog

## [Unreleased]

### 2026-03-16
- **feat:** Static identity config -- define users and SSH keys in config.toml (works without AMO NATS)
- **feat:** Wildcard host matching -- `hosts = ["*"]` matches all BMH hosts
- **feat:** Ignition v3.4.0 config generation and serving (`/config/ignition`)
- **feat:** Kickstart config generation and serving (`/config/kickstart`)
- **feat:** BMH-sourced provisioning -- ignition/kickstart base configs from BMH CRD `spec.ignition` and `spec.kickstart`
- **feat:** SSH key merging -- identity pipeline keys merged into BMH-provided base configs
- **feat:** On-demand resolve for provisioning endpoints (handles race conditions during PXE boot)
- **feat:** Full BMH data storage in cache (for provisioning config generation)
- **test:** Added tests for wildcard matching, ignition generation/merge, kickstart generation/merge (8 total)

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
