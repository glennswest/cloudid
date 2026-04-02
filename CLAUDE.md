# CLAUDE.md -- CloudIdOperator

## Core Rules

1. **All changes are approved.** Do not ask for confirmation before making changes. Execute the work.
2. **Every change must be committed to GitHub.** Commit early, commit often.
3. **Push after every logical unit of work.**
4. **Commit first, test after.** If tests fail, fix and commit the fix separately.
5. **CHANGELOG.md must be maintained.** Every change logged with date, description, and category.
6. **Documentation must stay current.** Code and docs ship together.
7. **This file is the work plan.** Update task lists as you progress.
8. **No sensitive information in commits.** Scan every change for secrets.
9. **Preserve context at all times.** Commit and push frequently.
10. **Follow semantic versioning.**

---

## Engineering Principles

### Self-Healing and Consistency

**This is the most critical principle.** When any error or inconsistency is discovered:

1. **Always add automated detection** -- if you find a bug, add code that detects the condition at startup and during operation
2. **Always add automated correction** -- if the condition is fixable, fix it automatically rather than requiring manual intervention
3. **Add consistency checks at every boundary** -- startup, cache rebuild, metadata response, NATS message processing
4. **Log what was detected and how it was fixed** -- never silently fix things
5. **Never leave a known failure mode without a self-healing path**

Examples:
- If BMH cache has stale IP mappings, refresh from mkube on cache rebuild
- If metadata cache returns no keys for a known host, trigger immediate cache rebuild
- If AMO NATS watcher disconnects, fall back to cached data and reconnect with exponential backoff
- If a metadata request comes from an unknown IP, return 404 but also trigger a BMH cache refresh (the host may be new)
- If the DNAT rule isn't working (host queries directly), still serve metadata correctly based on source IP

### Defensive Coding

- Validate all inputs (HTTP requests, NATS messages, mkube API responses)
- Use `Result<T>` everywhere, never `unwrap()` in production paths
- Timeouts on all network operations
- Graceful degradation: if AMO is down, serve from cache; if mkube is down, use cached BMH data
- Never panic on bad data from external sources
- Metadata endpoint must always respond (even if with empty/partial data) -- a stuck boot is worse than missing keys

### Testing

- Every bug fix must include a test that would have caught it
- Integration tests for metadata endpoint with mock BMH/user data
- Test cache rebuild under various failure conditions
- Test offline operation (NATS down, mkube down)

---

## Build Instructions

### Development (macOS)

```bash
cargo build
cargo test
cargo clippy
```

### Release Builds

```bash
# x86_64 Linux (standard Linux servers)
cargo build --release --target x86_64-unknown-linux-musl

# ARM64 Linux (MikroTik RouterOS, Storebase, ARM servers)
cargo build --release --target aarch64-unknown-linux-musl
```

Both produce fully static binaries suitable for `scratch` containers.

### Container Build

```bash
# Build binary for target platform
CGO_ENABLED=0 cargo build --release --target aarch64-unknown-linux-musl

# Build container
podman build --platform linux/arm64 -t registry.gt.lo:5000/cloudid:edge .

# Push to registry (mkube auto-updates)
podman push --tls-verify=false registry.gt.lo:5000/cloudid:edge
```

### Cross-Compile Targets

| Target | Platform | Use Case |
|--------|----------|----------|
| `x86_64-unknown-linux-musl` | x86_64 Linux | Standard servers, VMs |
| `aarch64-unknown-linux-musl` | ARM64 Linux | MikroTik RouterOS containers, Storebase, ARM servers |

Pure Rust, zero C dependencies, `FROM scratch` container.

### Dockerfile

```dockerfile
FROM scratch
COPY cloudid /cloudid
COPY config.toml /etc/cloudid/config.toml
EXPOSE 8090
ENTRYPOINT ["/cloudid", "serve", "--config", "/etc/cloudid/config.toml"]
```

### Deploy

CloudIdOperator is deployed as a container via mkube's deploy controller:

```bash
podman push --tls-verify=false registry.gt.lo:5000/cloudid:edge
```

### Host Agent Build

The `cloudid-agent` binary is built from the same crate with a feature flag:

```bash
# Agent binary for CoreOS/Linux hosts (x86_64)
cargo build --release --target x86_64-unknown-linux-musl --features agent --bin cloudid-agent

# Agent binary for ARM64 hosts
cargo build --release --target aarch64-unknown-linux-musl --features agent --bin cloudid-agent
```

---

## Version Management

Follow [Semantic Versioning 2.0.0](https://semver.org/).

### Version Locations
```
Cargo.toml  -> version = "X.Y.Z"
VERSION     -> X.Y.Z
```

---

## Architecture

### Key Directories
```
src/main.rs          -- Server entry point
src/config.rs        -- TOML config
src/metadata/        -- EC2-compatible metadata endpoint handlers
src/watcher/         -- AMO (NATS) and mkube (HTTP) watchers
src/cache.rs         -- In-memory metadata cache (IP -> keys precomputation)
src/resolve.rs       -- IP -> hostname -> HostAccess -> users -> SSH keys pipeline
src/provision.rs     -- Ignition/kickstart generation, template resolution + variable substitution
src/templates.rs     -- Template CRUD, file I/O, backup/restore, assignments, oneshot state
src/agent/           -- cloudid-agent binary (periodic key refresh)
tests/               -- Integration + unit tests
```

### Tech Stack
- **Language**: Rust (edition 2021)
- **Web**: axum 0.8, tokio
- **NATS**: async-nats
- **TLS**: rustls
- **CLI**: clap 4
- **Build**: musl static, `FROM scratch` container

### Key Commands
```bash
cargo build                                    # Dev build
cargo test                                     # Run tests
cargo build --release --target x86_64-unknown-linux-musl    # x86_64 release
cargo build --release --target aarch64-unknown-linux-musl   # ARM64 release
```

---

## Work Plan

### Current Version: `v0.2.0`

### Phase 1: Scaffolding
- [x] Initialize Cargo project
- [x] TOML config parsing (AMO NATS URL, mkube URL, metadata port, domain suffix)
- [x] axum server skeleton with health check
- [x] Define shared model types (User, Group, HostAccess, HostGroup -- same as AMO)
- [x] Tracing/logging setup
- [x] Startup self-checks: verify NATS connectivity, verify mkube reachability, log warnings if unavailable

### Phase 2: AMO Watcher
- [x] Connect to AMO's NATS JetStream KV buckets
- [x] Watch `AMO_USERS`, `AMO_GROUPS`, `AMO_HOSTACCESS`, `AMO_HOSTGROUPS` buckets
- [ ] Decrypt and verify NATS payloads (X25519 + Ed25519)
- [x] Maintain local in-memory copies of all identity data
- [x] Self-healing: if NATS disconnects, serve from cache, reconnect with exponential backoff
- [x] Self-healing: on reconnect, request full state dump to catch missed updates
- [ ] Consistency: compare local state hash with AMO on periodic intervals

### Phase 3: BMH Watcher
- [x] Watch mkube for BareMetalHost objects (`/api/v1/namespaces/{ns}/baremetalhosts?watch=true`)
- [x] Build and maintain IP -> hostname mapping table
- [x] Also query DHCP lease sources for additional IP -> hostname mappings
- [x] Self-healing: if mkube watch disconnects, reconnect and re-list
- [x] Self-healing: periodically refresh full BMH list to catch missed events
- [ ] Consistency: validate IP mappings against reverse DNS

### Phase 4: Metadata Endpoint
- [x] EC2-compatible metadata tree (`/latest/meta-data/*`)
- [x] IP resolution pipeline: source IP -> hostname -> HostAccess -> users -> SSH keys
- [x] SSH key aggregation per system user (root, core, etc.)
- [x] cloud-config user-data generation (`/latest/user-data`)
- [x] In-memory metadata cache (precomputed per IP)
- [x] Cache rebuild on AMO/BMH data changes (event-driven + minimum interval)
- [x] Self-healing: if a request hits unknown IP, log warning
- [x] Self-healing: if cache is empty, serve what we have and log warning (never block boot)
- [ ] Consistency: validate cached metadata against source data on periodic intervals

### Phase 5: Host Agent
- [x] `cloudid-agent` binary (separate bin target, same crate)
- [x] `refresh` subcommand: fetch keys from metadata endpoint, update authorized_keys
- [x] `authorized-keys <user>` subcommand: for sshd AuthorizedKeysCommand
- [x] `status` subcommand: show current metadata for this host
- [x] Ignition config serving (`/config/ignition`) -- base from BMH CRD, SSH keys merged
- [x] Kickstart config serving (`/config/kickstart`) -- base from BMH CRD, SSH keys merged
- [ ] Ignition DNAT rule + periodic timer systemd units
- [x] Self-healing: if metadata endpoint unreachable, keep existing keys (never delete working keys)

### Phase 6: Integration & Hardening
- [x] Unit tests for resolution pipeline
- [x] Integration tests with mock AMO data (29 tests in tests/integration_test.rs)
- [x] Test offline operation (NATS down, mkube down) — cache survival, rebuild with empty state, on-demand resolve fallback
- [x] Test unknown IP handling — cache miss, HTTP 404, is_unknown_ip
- [x] Test cache rebuild under load — concurrent reads, 100 users x 200 hosts large dataset
- [x] Container image (scratch)
- [ ] Deploy scripts + mkube pod manifest
- [ ] SSSD/PAM config examples for non-CoreOS Linux
- [ ] Performance testing (metadata response latency target: <5ms)

### In Progress
<!-- - [ ] (started YYYY-MM-DD) Task description -->
- [x] Container identity via namespace ownership — containers get SSH keys from namespace owner

### Completed
- [x] Phase 1: Project scaffolding (Cargo.toml, config, models, server skeleton)
- [x] Phase 2: AMO NATS watcher (connect, watch, initial load, reconnect)
- [x] Phase 3: BMH HTTP watcher (list, watch, DHCP leases, periodic refresh)
- [x] Phase 4: EC2-compatible metadata endpoint (all paths, cloud-config)
- [x] Phase 5: cloudid-agent binary (refresh, authorized-keys, status)
- [x] Phase 6: Dockerfile, unit tests, clippy clean
- [x] Static identity config (SSH keys in config.toml, works without AMO)
- [x] Ignition/kickstart provisioning (BMH CRD base config + SSH key merge)
- [x] Wildcard host matching for static access rules
- [x] Template system — REST API CRUD, PVC storage, backup/restore, assignments, oneshot, diagnostics

---

## Known Issues

### Cross-Network DNS Resolution Failure
- **Status**: Open -- workaround applied (cross-network DNS records in MicroDNS)
- **Problem**: Hosts on non-gt networks (e.g., g10) cannot resolve `registry.gt.lo` via systemd-resolved. DHCP option 15 (search domain `g10.lo`) causes systemd-resolved to scope the DNS server, refusing to forward queries for other domains like `gt.lo`.
- **Impact**: mkube-agent service fails to start on cross-network hosts (DNS wait loop times out at 60 iterations)
- **Root cause**: systemd-resolved + DHCP domain scoping. Direct DNS queries work (`nslookup registry.gt.lo 192.168.10.252` returns 192.168.200.3), but `getent hosts` and `resolvectl query` fail.
- **Workaround**: Add cross-network records to each MicroDNS instance
- **Proper fix**: One of: (1) MicroDNS DNS forwarding between zones, (2) DHCP option 119 with `~.` routing domain, (3) systemd-resolved drop-in in ignition templates with `Domains=~.`

### MicroDNS NXDOMAIN for AAAA on A-Only Records
- **Status**: Open -- may contribute to systemd-resolved failures
- **Problem**: When MicroDNS has an A record but no AAAA record for a name, it returns NXDOMAIN for the AAAA query instead of NOERROR with empty answer. systemd-resolved may treat this as the name not existing.
- **Impact**: Potentially worsens cross-network DNS failures

---

## Changes Needed

### Template Improvements
- [ ] Consider adding systemd-resolved drop-in to ignition templates (`Domains=~.`) to fix cross-network DNS
- [ ] Consider using registry IP (192.168.200.3) instead of hostname in agent-runner template as fallback
- [ ] agent-runner DNS wait loop should log which attempt it's on (currently silent during the 60 retries)

### MicroDNS Integration
- [ ] MicroDNS should return NOERROR (not NXDOMAIN) for AAAA queries when A record exists
- [ ] MicroDNS should support DNS forwarding between zones (g10 -> gt, etc.)
- [ ] Consider adding DHCP option 119 support to MicroDNS for routing domains

### Testing
- [ ] Integration test for cross-network DNS resolution in ignition templates
- [ ] Test agent-runner template on non-gt networks

---

## Lessons Learned

- **Self-healing is not optional.** Every known failure mode must have automated detection and correction. A bug that requires manual intervention will waste days. A bug that self-heals wastes minutes.
- **Never block a boot.** The metadata endpoint must always respond, even with partial data. A machine stuck in PXE boot loop is worse than a machine with missing SSH keys.
- **Cache is king.** Precompute everything. The metadata endpoint is on the hot path (every boot). Sub-5ms response time target.
- **Assume disconnection.** NATS can go down. mkube can go down. AMO can go down. CloudIdOperator must serve from cache indefinitely.
- **Log, don't crash.** Bad data from NATS, mkube, or AMO should be logged and skipped, never cause a panic.
- **systemd-resolved scopes DNS by DHCP domain.** When DHCP provides a search domain (option 15), systemd-resolved only uses that link's DNS server for queries matching the domain. Cross-domain queries silently fail unless a routing domain (`~.`) or fallback DNS is configured. Direct `nslookup <name> <server>` will work but `getent hosts` will not -- always test with `getent`/`resolvectl query`, not `nslookup`.

---

## Reminders

- Never leave work uncommitted
- Never skip the changelog
- Never let docs drift from code
- Never commit secrets
- Always add self-healing code when fixing bugs
- Always add consistency checks at boundaries
- Every bug fix needs a test
- Metadata endpoint must NEVER block or crash -- always respond
- Update this work plan before, during, and after tasks
