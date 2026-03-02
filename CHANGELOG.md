# Changelog

## [Unreleased]

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
