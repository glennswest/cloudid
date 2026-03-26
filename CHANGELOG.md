# Changelog

## [Unreleased]

### 2026-03-26
- **feat:** runner template: load `ublk_drv` kernel module at boot (`/etc/modules-load.d/ublk.conf`) — privileged containers can use `/dev/ublk-control` directly when module is loaded on host

### 2026-03-25
- **feat:** container identity now provisions root + owner username instead of hardcoded "admin" — containers get SSH keys for both root and the namespace owner's username (e.g. root + gwest)
- **fix:** container watcher pod IP deserialization — `podIP` (Kubernetes) vs `podIp` (camelCase) mismatch caused all pod IPs to deserialize as empty, resulting in container_ips=0
- **feat:** add `/api/v1/debug/state` diagnostic endpoint — dumps container IPs, namespace owners, BMH mappings, identity state, and cache entries

### 2026-03-24
- **feat:** set timezone to America/Chicago in all ignition templates via `/etc/localtime` symlink — affects agent-runner, agent-runner-install (inner ignition), runner, nextnfs, nfs-server
- **feat:** agent-runner-install ignition template — installs FCOS to /dev/sda with coreos-installer, then reboots into installed system with full agent-runner config (mkube-agent, registry, fuse, podman socket)
- **fix:** agent-runner and agent-runner-install: persistent container storage (`/var/data/agent-storage` bind to `/var/lib/containers`), persistent tmp (`/var/data/tmp`), removed pre-pull of rawhidedev/fedoradev images that blocked heartbeats for 10+ minutes
- **fix:** agent-runner-install: deduplicate SSH keys from cloudid metadata API — Ignition v3.4.0 rejects duplicate entries in `sshAuthorizedKeys`, and cloudid returns the same key under multiple indices (core, root)
- **feat:** agent-runner and agent-runner-install: load `ublk_drv` kernel module at boot (`/etc/modules-load.d/ublk.conf`) — io_uring is built-in (`CONFIG_IO_URING=y`), ublk is a module (`CONFIG_BLK_DEV_UBLK=m`)
- **feat:** agent-runner and agent-runner-install: load iSCSI modules at boot (`/etc/modules-load.d/iscsi.conf`) — `iscsi_tcp` (initiator), `iscsi_target_mod` + `target_core_mod` (target)
- **fix:** agent-runner and agent-runner-install: set `never-default=true` on enp5s0 NIC — prevents second NIC's default route from causing asymmetric routing (traffic in on enp3s0/192.168.10.10, out on enp5s0/192.168.10.20)

### 2026-03-23
- **feat:** Fedora Rawhide kickstart template for server2 — network install, SSH hardened, keys from CloudID
- **feat:** runner template: install iscsi-initiator-utils and liburing via rpm-ostree on first boot (needed by stormd for iSCSI targets and async I/O)

### 2026-03-22
- **fix:** agent-runner template pulls wrong architecture — added `--arch amd64` to all podman pull/run commands so x86_64 job runners don't pull ARM64 images
- **fix:** agent-runner: use host podman via socket instead of nested podman — mount `/run/podman/podman.sock`, enable `podman.socket`, set `CONTAINER_HOST` env var, remove nested storage and fuse workarounds

### 2026-03-21
- **fix:** agent-runner template now partitions and mounts /dev/sda as /var/data (XFS) — was using ephemeral tmpfs, disk sat unused
- **fix:** Mask NetworkManager-wait-online.service in all FCOS templates — iBFT phantom connection causes 60s timeout failure on servers with iSCSI firmware
- **docs:** agent-container-host-networking enhancement — host networking is mandatory for all container templates (not accessible without it)
- **feat:** nextnfs ignition template — deploys NextNFS server on FCOS with data disk formatting, XFS mount, and containerized NFSv4 server
- **docs:** Added Known Issues section to README.md (cross-network DNS resolution, agent-runner registry hostname)
- **docs:** Added Known Issues and Changes Needed sections to CLAUDE.md
- **docs:** Added lesson learned: systemd-resolved DHCP domain scoping behavior

### 2026-03-18
- **feat:** Container identity via namespace ownership — containers get SSH keys from namespace owner's identity (admin user with owner's keys, wheel group, sudo)
- **feat:** Container watcher — polls mkube for pod IPs and namespace owners (`vkube.io/owner` annotation), rebuilds metadata cache
- **feat:** `resolve_container()` — direct owner-to-keys mapping bypassing HostAccess rules, serves admin user with owner's SSH keys
- **feat:** K8s Pod/Namespace deserialize types for mkube API consumption
- **test:** 3 new tests: container resolution, unknown owner, disabled owner (20 total)
- **fix:** Normalize Ignition `inline` contents to `source` data URI format — Ignition v3.4.0 rejects `overwrite: true` with `inline` contents
- **fix:** Template extension resolution — BMH refs like `fcos/runner` now resolve to `runner.ign.json` on disk (tries `.ign.json`, `.ks`, `.yaml` extensions)

### 2026-03-17
- **feat:** stormd process manager integration — stormd as PID 1 with SSH access, structured logging, and restart policies
- **build:** Dockerfile updated to use stormd as entrypoint with cloudid as supervised process
- **build:** build.sh now includes stormd binary from `../stormd/target/aarch64-unknown-linux-musl/release/stormd`
- **build:** Added `deploy/stormd-config.toml` — stormd supervisor config for cloudid container
- **build:** Health probes updated to use stormd's `/api/v1/health` endpoint on port 9080
- **feat:** Template system — REST API for managing provisioning templates (CRUD, backup/restore)
- **feat:** Template assignments — assign templates to hosts via REST API or config
- **feat:** Oneshot templates — first-boot-only templates with completion tracking
- **feat:** Template variable substitution — `{{HOSTNAME}}`, `{{IP}}`, `{{INSTANCE_ID}}`, etc.
- **feat:** Template resolution priority — oneshot check → BMH spec → REST assignment → config fallback
- **feat:** Template format auto-detection from file extension (`.ign.json`, `.ks`, `.yaml`)
- **feat:** SSH key merging into templates — existing `merge_ignition`/`merge_kickstart` applied automatically
- **feat:** Template backup/restore — export all templates as JSON bundle, import to restore
- **feat:** Template diagnostics endpoint — `GET /config/template` shows resolved template for requesting host
- **feat:** PVC-backed template storage — templates survive restarts at `/var/lib/cloudid/templates/`
- **feat:** Image type organization — templates grouped by image type (`fcos/`, `fedora/`, `ubuntu/`)
- **feat:** Config-based template assignments — `[[templates.assignments]]` for bootstrap fallback
- **test:** Added 7 new tests (template CRUD, backup/restore, assignments, oneshot, variable substitution, format detection, image type extraction)
- **build:** PVC volume mount added to deploy manifest for `/var/lib/cloudid`
- **docs:** Comprehensive template system documentation in README.md

### 2026-03-17 (earlier)
- **fix:** gwest UID changed from 1000 to 1001 (1000 is reserved for `core` user on FCOS)
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
