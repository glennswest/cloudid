# CloudIdOperator

Afterburn-compatible metadata service for provisioning SSH keys and user accounts to bare metal hosts.

## Overview

CloudIdOperator serves SSH keys, hostnames, and user account data to CoreOS, Linux, and Windows machines on boot. It watches AMO (Associate Manager Operator) for user/group/access changes and mkube for BareMetalHost data, then resolves incoming metadata requests by source IP to determine which users have access to which hosts.

### What CloudIdOperator Does

- **EC2-Compatible Metadata Endpoint** -- serves instance metadata at `/latest/meta-data/` (port 8090), compatible with Afterburn on Fedora CoreOS
- **SSH Key Aggregation** -- resolves source IP -> hostname -> org -> HostAccess rules -> users -> SSH keys
- **cloud-config Generation** -- serves `/latest/user-data` with Unix user accounts, groups, sudo, and SSH keys
- **BMH Integration** -- watches mkube for BareMetalHost objects to maintain IP-to-hostname mappings
- **AMO Watcher** -- watches AMO via NATS for real-time user/group/access updates
- **Metadata Cache** -- precomputes per-host metadata for instant response on boot
- **Host Agent** -- optional `cloudid-agent` binary for periodic SSH key refresh on running hosts

### What CloudIdOperator Does NOT Do

- Does not manage users, groups, or orgs (that's AMO)
- Does not serve LDAP (that's AMO)
- Does not serve PXE boot files (that's pxemanager)

## Architecture

```
┌─────────────────────────────────────┐
│         CloudIdOperator              │
│                                     │
│  ┌─────────────┐  ┌──────────────┐ │
│  │ EC2 Metadata│  │   Watchers   │ │
│  │   :8090     │  │              │ │
│  └──────┬──────┘  │ AMO (NATS)   │ │
│         │         │ mkube (HTTP) │ │
│  ┌──────▼──────┐  └──────┬───────┘ │
│  │  Metadata   │◄────────┘         │
│  │   Cache     │                   │
│  │ (IP->keys)  │                   │
│  └─────────────┘                   │
└─────────────────────────────────────┘
         ▲
         │ HTTP (source IP identification)
         │
┌────────┴────────┐
│  CoreOS / Linux │
│  (Afterburn or  │
│   cloudid-agent)│
└─────────────────┘
```

## How It Works

### Boot-Time Flow

1. Host PXE boots via pxemanager, gets CoreOS with Ignition config
2. Ignition config includes iptables DNAT rule: `169.254.169.254:80` -> `cloudid.g10.lo:8090`
3. Afterburn queries `http://169.254.169.254/latest/meta-data/public-keys/` on first boot
4. CloudIdOperator receives request, identifies host by source IP
5. Resolves: source IP -> BMH hostname -> HostAccess rules -> users -> SSH keys
6. Returns aggregated SSH keys for each system user (root, core, etc.)
7. Afterburn writes keys to `~user/.ssh/authorized_keys.d/afterburn`

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
    - hosts[] contains "server1", OR
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
```

## EC2-Compatible Metadata Endpoint

Port 8090 serves the standard EC2 instance metadata tree:

```
GET /latest/meta-data/                          -> directory listing
GET /latest/meta-data/instance-id               -> "server1" (BMH name)
GET /latest/meta-data/hostname                  -> "server1.g10.lo"
GET /latest/meta-data/local-hostname            -> "server1"
GET /latest/meta-data/local-ipv4                -> "192.168.10.10"
GET /latest/meta-data/placement/availability-zone -> "gt" (cluster name)
GET /latest/meta-data/public-keys/              -> "0=root\n1=core\n2=gwest"
GET /latest/meta-data/public-keys/0/openssh-key -> SSH keys for root user
GET /latest/meta-data/public-keys/1/openssh-key -> SSH keys for core user
GET /latest/user-data                           -> cloud-config YAML
```

### User Data (cloud-config)

```yaml
#cloud-config
users:
  - name: gwest
    uid: "1000"
    groups: [wheel, engineering]
    shell: /bin/bash
    sudo: ALL=(ALL) NOPASSWD:ALL
    ssh_authorized_keys:
      - ssh-rsa AAAA...key gwest@macbook
```

## CoreOS Integration

### Ignition Config (DNAT Redirect)

Add to the Butane config for CoreOS machines:

```yaml
systemd:
  units:
    - name: cloudid-metadata-redirect.service
      enabled: true
      contents: |
        [Unit]
        Description=Redirect metadata queries to CloudIdOperator
        Before=afterburn-sshkeys@.service
        After=network-online.target

        [Service]
        Type=oneshot
        RemainAfterExit=yes
        ExecStart=/usr/sbin/iptables -t nat -A OUTPUT -d 169.254.169.254/32 -p tcp --dport 80 -j DNAT --to-destination 192.168.10.201:8090

        [Install]
        WantedBy=multi-user.target
```

Use `ignition.platform.id=aws` in kernel args and Afterburn handles SSH key provisioning natively.

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

## Linux Integration (SSSD)

For non-CoreOS Linux hosts, use SSSD pointing to AMO's LDAP server. CloudIdOperator is not needed in this case -- SSSD handles user resolution and SSH key retrieval via LDAP directly.

However, CloudIdOperator can still be used as a lightweight alternative (no SSSD dependency):

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

Configuration via environment or config file:
```
CLOUDID_METADATA_URL=http://cloudid.g10.lo:8090
CLOUDID_REFRESH_INTERVAL=5m
```

## Redundancy

- One CloudIdOperator instance per site
- Watches AMO via NATS for user/access changes
- Watches local mkube for BMH data
- Precomputes metadata cache in memory
- If AMO is unreachable, serves from last-known cache (offline-capable)
- Deploy controller (mkube) manages lifecycle and health checks
- Can run co-located with AMO or standalone

## Tech Stack

- **Language**: Rust (edition 2021)
- **Web**: axum 0.8, tokio
- **NATS**: async-nats (watches AMO KV buckets)
- **TLS**: rustls
- **CLI**: clap 4
- **Container**: `FROM scratch` (fully static musl binary)

## Build

```bash
# x86_64 Linux (musl static)
cargo build --release --target x86_64-unknown-linux-musl

# ARM64 Linux (MikroTik, Storebase)
cargo build --release --target aarch64-unknown-linux-musl

# Container (scratch)
podman build --platform linux/arm64 -t registry.gt.lo:5000/cloudid:edge .
podman push --tls-verify=false registry.gt.lo:5000/cloudid:edge

# macOS development
cargo build
cargo test
```

## Deploy

CloudIdOperator runs as a container managed by mkube's deploy controller.

```bash
# Push to registry -- mkube auto-updates the pod
podman push --tls-verify=false registry.gt.lo:5000/cloudid:edge
```

### Ports

| Port | Protocol | Purpose |
|------|----------|---------|
| 8090 | HTTP | EC2-compatible metadata endpoint |

## Configuration

```toml
# cloudid.toml
[server]
metadata_addr = "0.0.0.0:8090"

[amo]
nats_url = "nats://nats.gt.lo:4222"

[mkube]
url = "http://192.168.200.2:8082"
bmh_namespaces = ["g10", "g11"]

[metadata]
domain_suffix = ".g10.lo"
cache_rebuild_interval_secs = 30
dhcp_sources = ["http://dns.g10.lo:8080/api/v1/leases"]
```

## Relationship to AMO

CloudIdOperator reads identity data from AMO. AMO is the source of truth for users, orgs, groups, and access policies. CloudIdOperator transforms that data into machine-consumable metadata (SSH keys, cloud-config, EC2 metadata tree) and serves it to hosts on boot and periodically.

See the [AMO README](../amo/README.md) for the identity management system.
