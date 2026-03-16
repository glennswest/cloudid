# CloudIdOperator

Afterburn-compatible metadata service for provisioning SSH keys, user accounts, Ignition configs, and kickstart configs to bare metal hosts.

## Overview

CloudIdOperator serves SSH keys, hostnames, user account data, and provisioning configs to CoreOS and Linux machines on boot. It watches AMO (Associate Manager Operator) for user/group/access changes and mkube for BareMetalHost data, then resolves incoming metadata requests by source IP to determine which users have access to which hosts.

### What CloudIdOperator Does

- **EC2-Compatible Metadata Endpoint** -- serves instance metadata at `/latest/meta-data/` (port 8090), compatible with Afterburn on Fedora CoreOS
- **SSH Key Aggregation** -- resolves source IP -> hostname -> HostAccess rules -> users -> SSH keys
- **cloud-config Generation** -- serves `/latest/user-data` with Unix user accounts, groups, sudo, and SSH keys
- **Ignition Config** -- serves `/config/ignition` with Ignition v3.4.0 JSON, merging SSH keys from identity into BMH-provided base configs
- **Kickstart Config** -- serves `/config/kickstart` with kickstart text, merging SSH keys from identity into BMH-provided base configs
- **Static Identity** -- SSH keys from `.pub` files in config, works without AMO NATS for bootstrap
- **BMH Integration** -- watches mkube for BareMetalHost objects to maintain IP-to-hostname mappings
- **AMO Watcher** -- watches AMO via NATS for real-time user/group/access updates
- **Metadata Cache** -- precomputes per-host metadata for instant response on boot
- **DHCP Route Auto-Config** -- discovers data networks from mkube and configures DHCP option 121 on each MicroDNS so hosts route `169.254.169.254` to CloudID via their gateway
- **Host Agent** -- optional `cloudid-agent` binary for periodic SSH key refresh on running hosts

### What CloudIdOperator Does NOT Do

- Does not manage users, groups, or orgs (that's AMO)
- Does not serve LDAP (that's AMO)
- Does not serve PXE boot files (that's pxemanager)

## Architecture

```
┌──────────────────────────────────────────┐
│           CloudIdOperator                │
│                                          │
│  ┌──────────────┐  ┌─────────────────┐  │
│  │ EC2 Metadata │  │    Watchers     │  │
│  │   :8090      │  │                 │  │
│  ├──────────────┤  │ AMO (NATS)      │  │
│  │ /config/     │  │ mkube (HTTP)    │  │
│  │  ignition    │  │ Networks (HTTP) │  │
│  │  kickstart   │  └────────┬────────┘  │
│  └──────┬───────┘           │           │
│  ┌──────▼───────┐           │           │
│  │  Metadata    │◄──────────┘           │
│  │   Cache      │                       │
│  │ (IP->keys)   │  ┌─────────────────┐  │
│  └──────────────┘  │ DHCP Route Mgr  │  │
│                    │ (MicroDNS API)  │  │
│                    └─────────────────┘  │
└──────────────────────────────────────────┘
         ▲
         │ 169.254.169.254:80 (DNAT via MikroTik)
         │
┌────────┴────────┐
│  CoreOS / Linux │
│  (Afterburn or  │
│   cloudid-agent)│
└─────────────────┘
```

## How It Works

### Boot-Time Flow

1. Host PXE boots, gets DHCP lease with option 121 route (`169.254.169.254/32 via gateway`)
2. MikroTik DNAT redirects `169.254.169.254:80` to `CloudID:8090`
3. For CoreOS: Afterburn queries `http://169.254.169.254/latest/meta-data/public-keys/`
4. For Ignition: fetches config from `http://169.254.169.254/config/ignition`
5. For kickstart: anaconda fetches from `http://169.254.169.254/config/kickstart`
6. CloudIdOperator identifies host by source IP, resolves access rules, returns metadata

### Metadata Discovery (169.254.169.254)

Hosts discover CloudID at the standard cloud metadata address without kernel args:

1. **DHCP option 121** -- CloudID auto-configures MicroDNS on each data network to push a static route for `169.254.169.254/32` via the network gateway
2. **MikroTik DNAT** -- one global rule redirects `169.254.169.254:80` to CloudID at `192.168.200.20:8090`
3. **Network discovery** -- CloudID queries mkube `/api/v1/networks`, finds all `type=data` networks with DHCP enabled, and configures routes automatically

No per-network router configuration. Adding a new data network in mkube is all that's needed.

### Resolution Pipeline

```
HTTP Request (source IP: 192.168.10.10)
    │
    ▼
Step 1: IP -> Hostname
    ip_host_map lookup: 192.168.10.10 -> "server1"
    (populated from BMH watch + DHCP leases)
    │
    ▼
Step 2: Hostname -> HostAccess
    Find all HostAccess rules where:
    - hosts[] contains "server1" (or "*" wildcard), OR
    - hostGroups[] references a group containing "server1", OR
    - hostSelectors[] match server1's BMH labels
    │
    ▼
Step 3: Collect Users
    Expand subjects (users + groups) from matching HostAccess rules
    Filter to enabled users only
    Collect SSH public keys per ssh_user (root, core)
    │
    ▼
Step 4: Serve Metadata
    /latest/meta-data/public-keys/0/openssh-key -> root's aggregated keys
    /latest/meta-data/public-keys/1/openssh-key -> core's aggregated keys
    /latest/user-data -> cloud-config YAML with user accounts
    /config/ignition -> Ignition v3.4.0 JSON with users + SSH keys
    /config/kickstart -> kickstart text with users + SSH keys
```

## Endpoints

### EC2-Compatible Metadata (port 8090)

```
GET /latest/meta-data/                          -> directory listing
GET /latest/meta-data/instance-id               -> "server1" (BMH name)
GET /latest/meta-data/hostname                  -> "server1.g10.lo"
GET /latest/meta-data/local-hostname            -> "server1"
GET /latest/meta-data/local-ipv4                -> "192.168.10.10"
GET /latest/meta-data/placement/availability-zone -> "gt" (cluster name)
GET /latest/meta-data/public-keys/              -> "0=root\n1=core\n"
GET /latest/meta-data/public-keys/0/openssh-key -> SSH keys for root user
GET /latest/meta-data/public-keys/1/openssh-key -> SSH keys for core user
GET /latest/user-data                           -> cloud-config YAML
GET /health                                     -> "ok"
```

### Provisioning Endpoints

```
GET /config/ignition   -> Ignition v3.4.0 JSON (Content-Type: application/json)
GET /config/kickstart  -> Kickstart text (Content-Type: text/plain)
```

Both endpoints resolve the host by source IP and merge SSH keys from the identity pipeline into the base config from the BMH CRD. If no base config exists on the BMH, a default is generated.

### BMH CRD Provisioning Config

BareMetalHost objects in mkube can carry base provisioning configs:

```yaml
apiVersion: v1
kind: BareMetalHost
metadata:
  name: server1
  namespace: g10
spec:
  hostname: server1
  ip: 192.168.10.10
  bootMacAddress: "ac:1f:6b:8a:a7:9c"
  image: fedora-coreos-40
  network: g10
  ignition:
    ignition:
      version: "3.4.0"
    storage:
      files:
        - path: /etc/sysctl.d/custom.conf
          contents:
            source: "data:,vm.swappiness=10"
          mode: 420
```

CloudID reads `spec.ignition` (JSON) or `spec.kickstart` (text) from the BMH, merges SSH keys and user accounts from the identity pipeline, and serves the result. The BMH carries platform config (storage, network, packages); CloudID injects identity (users, SSH keys, sudo).

### User Data (cloud-config)

```yaml
#cloud-config
users:
  - name: gwest
    uid: "1000"
    groups: [wheel]
    shell: /bin/bash
    sudo: ALL=(ALL) NOPASSWD:ALL
    ssh_authorized_keys:
      - ssh-rsa AAAA...key gwest@macbook
```

## Configuration

```toml
[server]
metadata_addr = "0.0.0.0:8090"

[amo]
nats_url = "nats://nats.gt.lo:4222"

[mkube]
url = "http://192.168.200.2:8082"
bmh_namespaces = ["g10", "g11"]

[metadata]
domain_suffix = ".g10.lo"
availability_zone = "gt"
cache_rebuild_interval_secs = 30
dhcp_sources = ["http://dns.g10.lo:8080/api/v1/leases"]

# Static identity -- works without AMO NATS
[[static_users]]
name = "gwest"
uid = 1000
gid = 1000
shell = "/bin/bash"
groups = ["wheel"]
ssh_key_files = ["/etc/cloudid/gwest.pub"]

# Grant access to all BMH hosts as root and core
[[static_host_access]]
ssh_users = ["root", "core"]
hosts = ["*"]
users = ["gwest"]
sudo = true
```

### Static Identity

SSH keys are loaded from `.pub` files (authorized_keys format, one key per line). This provides a bootstrap path that works without AMO NATS:

- Define users with `[[static_users]]` and reference their `.pub` key files
- Define access rules with `[[static_host_access]]` -- use `hosts = ["*"]` for all BMH hosts
- When AMO connects, its data overlays on top of static config
- Multiple users supported, each with their own key files and access rules

### Network Auto-Discovery

CloudID discovers data networks from mkube automatically -- no network config needed:

1. Queries `GET /api/v1/networks` from mkube on startup
2. Filters for `type=data` networks with DHCP enabled
3. Calls each network's MicroDNS to add DHCP option 121 (classless static route) for `169.254.169.254/32 via gateway`
4. Re-checks every 5 minutes (self-healing)

## CoreOS Integration

### Periodic Key Refresh

For ongoing SSH key updates without rebooting:

```yaml
systemd:
  units:
    - name: cloudid-refresh.timer
      enabled: true
      contents: |
        [Unit]
        Description=Refresh SSH keys from CloudIdOperator

        [Timer]
        OnBootSec=30s
        OnUnitActiveSec=5min

        [Install]
        WantedBy=timers.target
    - name: cloudid-refresh.service
      contents: |
        [Unit]
        Description=Refresh SSH keys

        [Service]
        Type=oneshot
        ExecStart=/usr/local/bin/cloudid-agent refresh
```

## Linux Integration

For non-CoreOS Linux hosts, use sshd's AuthorizedKeysCommand:

```
# /etc/ssh/sshd_config
AuthorizedKeysCommand /usr/local/bin/cloudid-agent authorized-keys %u
AuthorizedKeysCommandUser nobody
```

## Host Agent (`cloudid-agent`)

Optional static binary that runs on hosts for SSH key management:

```bash
# Fetch and install SSH keys for all users
cloudid-agent refresh

# Query authorized keys for a specific user (for sshd AuthorizedKeysCommand)
cloudid-agent authorized-keys root

# Show what metadata would be served for this host
cloudid-agent status
```

Configuration via environment:
```
CLOUDID_METADATA_URL=http://169.254.169.254
```

## Deploy

CloudIdOperator runs as a container managed by mkube's deploy controller at `192.168.200.20` on the gt network.

```bash
# Build
cargo build --release --target x86_64-unknown-linux-musl

# Container
podman build --platform linux/arm64 -t registry.gt.lo:5000/cloudid:edge .
podman push --tls-verify=false registry.gt.lo:5000/cloudid:edge
```

### Prerequisites

One-time MikroTik DNAT rule (already configured):
```
/ip firewall nat add chain=dstnat dst-address=169.254.169.254 protocol=tcp dst-port=80 action=dst-nat to-addresses=192.168.200.20 to-ports=8090 comment="cloudid metadata redirect"
```

### Ports

| Port | Protocol | Purpose |
|------|----------|---------|
| 8090 | HTTP | EC2-compatible metadata + provisioning endpoints |

## Redundancy

- One CloudIdOperator instance per site
- Watches AMO via NATS for user/access changes
- Watches local mkube for BMH data
- Precomputes metadata cache in memory
- If AMO is unreachable, serves from static config and last-known cache (offline-capable)
- Deploy controller (mkube) manages lifecycle and health checks

## Tech Stack

- **Language**: Rust (edition 2021)
- **Web**: axum 0.8, tokio
- **NATS**: async-nats (watches AMO KV buckets)
- **TLS**: rustls
- **CLI**: clap 4
- **Container**: `FROM scratch` (fully static musl binary)

## Relationship to AMO

CloudIdOperator reads identity data from AMO. AMO is the source of truth for users, orgs, groups, and access policies. CloudIdOperator transforms that data into machine-consumable metadata (SSH keys, cloud-config, Ignition, kickstart) and serves it to hosts on boot and periodically.
