# CloudIdOperator

Afterburn-compatible metadata service for provisioning SSH keys, user accounts, Ignition configs, kickstart configs, and configurable templates to bare metal hosts.

## Overview

CloudIdOperator serves SSH keys, hostnames, user account data, and provisioning configs to CoreOS and Linux machines on boot. It watches AMO (Associate Manager Operator) for user/group/access changes and mkube for BareMetalHost data, then resolves incoming metadata requests by source IP to determine which users have access to which hosts.

### What CloudIdOperator Does

- **EC2-Compatible Metadata Endpoint** -- serves instance metadata at `/latest/meta-data/` (port 8090), compatible with Afterburn on Fedora CoreOS
- **SSH Key Aggregation** -- resolves source IP -> hostname -> HostAccess rules -> users -> SSH keys
- **cloud-config Generation** -- serves `/latest/user-data` with Unix user accounts, groups, sudo, and SSH keys
- **Ignition Config** -- serves `/config/ignition` with Ignition v3.4.0 JSON, merging SSH keys from identity into BMH-provided base configs
- **Kickstart Config** -- serves `/config/kickstart` with kickstart text, merging SSH keys from identity into BMH-provided base configs
- **Template System** -- configurable templates define what a host becomes at boot (agent runner, NFS server, etc.) without changing Rust code. REST API for CRUD, assignments, oneshot mode, backup/restore
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
┌────────────────────────────────────────────────┐
│              CloudIdOperator                    │
│                                                │
│  ┌──────────────┐  ┌─────────────────┐         │
│  │ EC2 Metadata │  │    Watchers     │         │
│  │   :8090      │  │                 │         │
│  ├──────────────┤  │ AMO (NATS)      │         │
│  │ /config/     │  │ mkube (HTTP)    │         │
│  │  ignition    │  │ Networks (HTTP) │         │
│  │  kickstart   │  └────────┬────────┘         │
│  ├──────────────┤           │                  │
│  │ /api/v1/     │           │                  │
│  │  templates   │  ┌────────▼────────┐         │
│  │  assignments │  │  Metadata Cache │         │
│  │  oneshot     │  │  (IP->keys)     │         │
│  └──────┬───────┘  └────────────────┘          │
│         │                                      │
│  ┌──────▼───────┐  ┌─────────────────┐         │
│  │  Template    │  │ DHCP Route Mgr  │         │
│  │   Store      │  │ (MicroDNS API)  │         │
│  │ (PVC disk)   │  └─────────────────┘         │
│  └──────────────┘                              │
└────────────────────────────────────────────────┘
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
6. CloudIdOperator identifies host by source IP, checks for a matching template, resolves access rules, merges SSH keys, and returns the config
7. If the template is **oneshot**, the host calls `POST /config/provisioned` after first boot and subsequent boots return no template (boot from local disk)

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

Both endpoints resolve the host by source IP. If a template is assigned to the host, the template content is served with variable substitution and SSH key merging applied. If no template matches, the default behavior generates a config from the identity pipeline (or uses a BMH base config if present).

### Template REST API

```
# Template CRUD
GET    /api/v1/templates                           -> list all templates
GET    /api/v1/templates/{image_type}              -> list templates for an image type
GET    /api/v1/templates/{image_type}/{name}       -> get template (content + metadata)
PUT    /api/v1/templates/{image_type}/{name}       -> create or update template
DELETE /api/v1/templates/{image_type}/{name}       -> delete template

# Backup & Restore
GET    /api/v1/templates/backup                    -> export all templates as JSON bundle
POST   /api/v1/templates/restore                   -> import templates from JSON bundle

# Host-to-Template Assignments
GET    /api/v1/assignments                         -> list all assignments
PUT    /api/v1/assignments/{hostname}              -> assign a template to a host
DELETE /api/v1/assignments/{hostname}              -> remove assignment

# Oneshot Management
POST   /config/provisioned                         -> host marks oneshot complete (by source IP)
GET    /api/v1/oneshot                             -> list all oneshot completion states
DELETE /api/v1/oneshot/{hostname}                  -> reset oneshot for re-provisioning

# Diagnostics
GET    /config/template                            -> template info for requesting host (by source IP)
```

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

## Template System

Templates make the same base ISO universal -- a host becomes an agent runner, NFS server, or GPU worker based on its template assignment, without changing Rust code. Templates are managed via REST API and stored on a PVC.

### Concepts

- **Template** -- a provisioning config file (Ignition JSON, kickstart, or cloud-config YAML) with variable placeholders, stored on disk at `/var/lib/cloudid/templates/{image_type}/{name}`
- **Image type** -- a directory grouping templates by OS family (`fcos/`, `fedora/`, `ubuntu/`). Templates under `fcos/` work for any FCOS version.
- **Assignment** -- maps a hostname to a specific template. Can come from the BMH CRD, REST API, or config file.
- **Oneshot mode** -- template is served on first boot only. After the host calls `POST /config/provisioned`, subsequent boots return no template (host boots from local disk).
- **Forever mode** -- template is served on every boot.

### Storage Layout (PVC)

```
/var/lib/cloudid/
  templates/
    fcos/
      agent-runner.ign.json           # Ignition template
      agent-runner.ign.json.meta.json # Metadata (mode, timestamps)
      nfs-server.ign.json
      gpu-worker.ign.json
    fedora/
      base-server.ks                  # Kickstart template
      nfs-server.ks
    ubuntu/
      base-server.yaml                # Cloud-config template
  assignments.json                    # Host-to-template assignments
  oneshot.json                        # Oneshot completion state
```

### Template Format

Templates use `{{VARIABLE}}` placeholders -- simple string replacement, no template engine:

| Variable | Value | Example |
|----------|-------|---------|
| `{{HOSTNAME}}` | FQDN | `server1.g10.lo` |
| `{{SHORT_HOSTNAME}}` | Short name | `server1` |
| `{{IP}}` | Host IP address | `192.168.10.10` |
| `{{INSTANCE_ID}}` | Instance identifier | `server1` |
| `{{AVAILABILITY_ZONE}}` | AZ string | `gt` |
| `{{HOSTNAME_ENCODED}}` | URL-encoded FQDN | `server1.g10.lo` |
| `{{DOMAIN_SUFFIX}}` | Domain suffix | `.g10.lo` |
| `{{TEMPLATE_NAME}}` | Template filename | `agent-runner.ign.json` |

Format is auto-detected by file extension:

| Extension | Format | Content-Type |
|-----------|--------|-------------|
| `.ign.json` | Ignition | `application/json` |
| `.ks` | Kickstart | `text/plain` |
| `.yaml` | Cloud-config | `text/yaml` |

SSH keys and users are merged automatically by the existing `merge_ignition` / `merge_kickstart` functions -- template authors never hardcode SSH keys.

### Template Resolution Priority

When a host requests a provisioning config, CloudID checks these sources in order:

1. **Oneshot check** -- if the host already completed a oneshot template, return nothing (boot from local disk)
2. **BMH `spec.template`** -- the BareMetalHost CRD can specify a template (format: `fcos/agent-runner.ign.json` or just `agent-runner.ign.json` with image type derived from `spec.image`)
3. **REST API assignment** -- assignments created via `PUT /api/v1/assignments/{hostname}`, stored in `/var/lib/cloudid/assignments.json`
4. **Config-based assignment** -- `[[templates.assignments]]` in config.toml (bootstrap fallback)
5. **No match** -- fall back to default behavior (generate config from identity pipeline only)

### Creating Templates

```bash
# Create an Ignition template for FCOS agent runners
curl -s -X PUT http://cloudid:8090/api/v1/templates/fcos/agent-runner.ign.json \
  -H 'Content-Type: application/json' \
  -d '{
    "mode": "oneshot",
    "content": "{\"ignition\":{\"version\":\"3.4.0\"},\"storage\":{\"disks\":[{\"device\":\"/dev/sda\",\"wipeTable\":true,\"partitions\":[{\"label\":\"root\",\"sizeMiB\":0}]}],\"filesystems\":[{\"path\":\"/\",\"device\":\"/dev/disk/by-partlabel/root\",\"format\":\"xfs\",\"wipeFilesystem\":true}],\"files\":[{\"path\":\"/etc/hostname\",\"mode\":420,\"overwrite\":true,\"contents\":{\"source\":\"data:,{{HOSTNAME_ENCODED}}\"}}]},\"systemd\":{\"units\":[{\"name\":\"cloudid-provisioned.service\",\"enabled\":true,\"contents\":\"[Unit]\\nDescription=Mark oneshot provisioning complete\\nAfter=multi-user.target\\nConditionFirstBoot=yes\\n\\n[Service]\\nType=oneshot\\nExecStart=/usr/bin/curl -s -X POST http://169.254.169.254/config/provisioned\\n\\n[Install]\\nWantedBy=multi-user.target\"}]}}"
  }'
```

```bash
# Create a kickstart template for Fedora
curl -s -X PUT http://cloudid:8090/api/v1/templates/fedora/base-server.ks \
  -H 'Content-Type: application/json' \
  -d '{
    "mode": "oneshot",
    "content": "#version=RHEL9\nlang en_US.UTF-8\nkeyboard us\ntimezone UTC --utc\nnetwork --hostname={{HOSTNAME}}\nrootpw --lock\n\nautopart --type=lvm\nclearpart --all --initlabel\n\n%packages\n@core\n%end\n\nreboot\n"
  }'
```

### Assigning Templates to Hosts

There are three ways to assign templates:

**1. BMH CRD (highest priority)**

```yaml
apiVersion: v1
kind: BareMetalHost
metadata:
  name: server1
  namespace: g10
spec:
  hostname: server1
  image: fcos-44
  template: fcos/agent-runner.ign.json   # explicit image_type/name
  # or just: template: agent-runner.ign.json  # image type derived from spec.image
```

**2. REST API assignment**

```bash
# Assign template to a specific host
curl -s -X PUT http://cloudid:8090/api/v1/assignments/server1 \
  -H 'Content-Type: application/json' \
  -d '{"image_type": "fcos", "template": "agent-runner.ign.json"}'

# List all assignments
curl -s http://cloudid:8090/api/v1/assignments

# Remove assignment
curl -s -X DELETE http://cloudid:8090/api/v1/assignments/server1
```

**3. Config file (lowest priority, bootstrap fallback)**

```toml
[[templates.assignments]]
hosts = ["server1", "server2"]
template = "fcos/agent-runner.ign.json"

# Wildcard: assign to all hosts
[[templates.assignments]]
hosts = ["*"]
template = "fcos/agent-runner.ign.json"
```

### Oneshot Templates

Oneshot templates are served on first boot only. After the host completes provisioning, it calls `POST /config/provisioned` and subsequent boots receive no template (the host boots from local disk).

```bash
# Host marks itself as provisioned (called by the host itself, identified by source IP)
curl -s -X POST http://169.254.169.254/config/provisioned

# View all oneshot completion states
curl -s http://cloudid:8090/api/v1/oneshot

# Reset a host for re-provisioning (serves the template again on next boot)
curl -s -X DELETE http://cloudid:8090/api/v1/oneshot/server1
```

For FCOS, include a systemd oneshot unit in the template that calls `/config/provisioned` on first boot:

```json
{
  "systemd": {
    "units": [{
      "name": "cloudid-provisioned.service",
      "enabled": true,
      "contents": "[Unit]\nDescription=Mark oneshot complete\nAfter=multi-user.target\nConditionFirstBoot=yes\n\n[Service]\nType=oneshot\nExecStart=/usr/bin/curl -s -X POST http://169.254.169.254/config/provisioned\n\n[Install]\nWantedBy=multi-user.target"
    }]
  }
}
```

### Backup and Restore

Export all templates as a JSON bundle for backup or migration:

```bash
# Export all templates
curl -s http://cloudid:8090/api/v1/templates/backup > templates-backup.json

# Import templates (overwrites existing with same name)
curl -s -X POST http://cloudid:8090/api/v1/templates/restore \
  -H 'Content-Type: application/json' \
  -d @templates-backup.json
```

Bundle format:

```json
{
  "version": 1,
  "exported_at": "2026-03-17T03:00:00Z",
  "templates": [
    {
      "image_type": "fcos",
      "name": "agent-runner.ign.json",
      "format": "ignition",
      "mode": "oneshot",
      "content": "{ ... }"
    }
  ]
}
```

### Diagnostics

Check what template would be served for a host:

```bash
# From the host itself (uses source IP)
curl -s http://169.254.169.254/config/template

# Response:
{
  "ip": "192.168.10.10",
  "hostname": "server1",
  "template": "fcos/agent-runner.ign.json",
  "source": "rest_assignment",
  "oneshot_completed": false
}
```

### Image Type Matching

When a BMH specifies `spec.template: agent-runner.ign.json` (without an image type prefix), CloudID extracts the base image type from `spec.image`:

| BMH `spec.image` | Extracted type | Template path |
|-------------------|---------------|---------------|
| `fcos-44` | `fcos` | `fcos/agent-runner.ign.json` |
| `fedora-9` | `fedora` | `fedora/agent-runner.ign.json` |
| `ubuntu-24.04` | `ubuntu` | `ubuntu/agent-runner.ign.json` |

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

[metadata]
domain_suffix = ".g10.lo"
availability_zone = "gt"
cache_rebuild_interval_secs = 30
dhcp_sources = ["http://dns.g10.lo:8080/api/v1/leases"]

# Template system (PVC mount point)
[templates]
data_dir = "/var/lib/cloudid"

# Optional: config-based template assignments (bootstrap fallback)
# [[templates.assignments]]
# hosts = ["*"]
# template = "fcos/agent-runner.ign.json"

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

CloudIdOperator runs as a container managed by mkube's deploy controller at `192.168.200.20` on the gt network. A PVC is mounted at `/var/lib/cloudid` for persistent template storage, assignments, and oneshot state.

```bash
# Build
cargo build --release --target x86_64-unknown-linux-musl

# Container
podman build --platform linux/arm64 -t registry.gt.lo:5000/cloudid:edge .
podman push --tls-verify=false registry.gt.lo:5000/cloudid:edge
```

### Volumes

| Mount | Source | Purpose |
|-------|--------|---------|
| `/etc/cloudid` | ConfigMap `cloudid-config` | Config file and SSH public key files |
| `/var/lib/cloudid` | PVC `cloudid-data` | Templates, assignments, oneshot state |

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

## Known Issues

### Cross-Network DNS Resolution (systemd-resolved + DHCP domain scoping)

**Affected**: Any host on a non-gt network (e.g., g10) that needs to resolve `registry.gt.lo` or other cross-network hostnames.

**Symptom**: `getent hosts registry.gt.lo` fails, causing the mkube-agent DNS wait loop to time out. Direct queries (`nslookup registry.gt.lo 192.168.10.252`) succeed, but systemd-resolved refuses to use the link DNS server for domains outside the DHCP-provided search domain.

**Root cause**: When DHCP provides a search domain (e.g., `g10.lo` via option 15), systemd-resolved scopes the link's DNS server to that domain. Queries for other domains (e.g., `gt.lo`) are not forwarded to that DNS server, even though `+DefaultRoute: yes` is set. There is no fallback DNS server configured, so the query fails.

**Current workaround**: Add cross-network DNS records to each network's MicroDNS (e.g., add a `gt.lo` zone or individual records like `registry.gt.lo` to g10's MicroDNS). The MicroDNS NXDOMAIN-vs-NOERROR behavior for AAAA queries on A-only records may also need fixing.

**Proper fix options**:
1. Configure MicroDNS DNS forwarding so each instance can resolve other network zones
2. Add DHCP option 119 (search domain list) with `~.` routing domain to tell systemd-resolved to route all queries through the link DNS server
3. Remove the DHCP domain option entirely (makes the DNS server handle all queries)
4. Add a systemd-resolved drop-in in ignition templates that sets `Domains=~.` globally

### Agent Runner Template Uses Hostname for Registry

**Affected**: `agent-runner.ign.json` template references `registry.gt.lo` by hostname.

**Symptom**: On networks where `registry.gt.lo` cannot be resolved, the agent service fails to start (stuck in DNS wait loop, then image pull failures).

**Mitigation**: Ensure cross-network DNS resolution works (see above), or change the template to use the registry IP directly (`192.168.200.3`). Using the IP is less maintainable but eliminates the DNS dependency.

## Relationship to AMO

CloudIdOperator reads identity data from AMO. AMO is the source of truth for users, orgs, groups, and access policies. CloudIdOperator transforms that data into machine-consumable metadata (SSH keys, cloud-config, Ignition, kickstart) and serves it to hosts on boot and periodically.
