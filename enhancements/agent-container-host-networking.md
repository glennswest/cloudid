# Agent Container Host Networking

## Rule

**All containers in ignition templates MUST use `--network=host`.** This is the default for every template, not an option. Without host networking, the container is not accessible from the LAN and cannot be identified by its real IP.

## Why Host Networking Is Required

### 1. Containers Are Not Accessible Without It

With default podman networking (slirp4netns/pasta), the container gets a private NAT'd IP. Nothing on the LAN can reach the container's services unless explicit `-p` port mappings are added. Port mapping is fragile, adds complexity, and breaks when ports conflict with host services.

With `--network=host`, every port the container listens on is directly reachable at the host's LAN IP. This is how bare metal services should work.

### 2. Source IP Authentication

mkube and CloudID identify hosts by source IP. When a container at `192.168.10.10` makes outbound requests:
- **With NAT**: mkube sees a translated IP and returns `404: no BMH found`
- **With host networking**: mkube sees the real IP and matches the BMH

This affects agent job polling, metadata requests, and any API that authenticates by source IP.

### 3. Nested Container LAN Access

Containers that run podman-in-podman (like the mkube-agent build runner) spawn child containers that also need LAN access. With host networking on the outer container, nested containers inherit direct network access. With NAT, they're double-NAT'd and routing becomes unreliable.

### 4. DNS Resolution

With host networking, the container uses the host's DNS configuration (systemd-resolved). With NAT, podman generates its own resolv.conf which may not match the host's DNS setup, causing resolution failures for internal hostnames.

## Applies To All Templates

Every FCOS ignition template that runs a container must use `--network host`:

| Template | Service | Container |
|----------|---------|-----------|
| `agent-runner.ign.json` | `mkube-agent.service` | `mkube-agent:edge` |
| `nextnfs.ign.json` | `nextnfs.service` | `nextnfs:latest` |
| `nfs-server.ign.json` | `nfs-server.service` | `nfs-server:latest` |
| Any future template | Any container service | Any container |

## Comparison

| Aspect | NAT (default podman) | `--network=host` |
|--------|----------------------|-------------------|
| Container IP | Private (10.0.2.x) | Same as host (192.168.10.10) |
| Outbound source IP | Translated | Real host IP |
| Inbound access | Requires `-p` mapping | All ports directly accessible |
| mkube identification | Fails (wrong IP) | Works |
| CloudID metadata | Fails (wrong IP) | Works |
| Nested containers | Double NAT | Direct LAN access |
| DNS | Podman-generated resolv.conf | Host's systemd-resolved |

## Standard Container Run Pattern

### Privileged (podman-in-podman, e.g., mkube-agent)

```bash
podman run \
  --name <service> \
  --rm \
  --network host \
  --privileged \
  -v /var/data:/data \
  -v /var/data/agent-storage:/var/lib/containers \
  -e MKUBE_API=http://192.168.200.2:8082 \
  --pull=always \
  registry.gt.lo:5000/<image>:edge
```

### Non-privileged (e.g., nextnfs, nfs-server)

```bash
podman run \
  --name <service> \
  --rm \
  --network host \
  -v /var/data/nfs:/export:z \
  --pull=always \
  registry.gt.lo:5000/<image>:latest
```

## Standard Systemd Unit Pattern

```ini
[Unit]
Description=<service> (container)
After=network-online.target <storage-deps>
Wants=network-online.target
Requires=<storage-deps>

[Service]
Type=simple
Restart=always
RestartSec=5
ExecStartPre=/bin/bash -c 'echo Waiting for DNS...; for i in $(seq 1 60); do getent hosts registry.gt.lo && exit 0; sleep 2; done; echo DNS timeout; exit 1'
ExecStartPre=-/usr/bin/podman rm -f <service>
ExecStart=/usr/bin/podman run --name <service> --rm --network host ...
ExecStop=/usr/bin/podman stop <service>
TimeoutStartSec=300
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

Key elements:
- DNS wait loop before first pull (cross-network DNS may be slow)
- Remove stale container before starting (`-` prefix ignores failure)
- `Restart=always` + `RestartSec=5` for automatic recovery
- `TimeoutStartSec=300` for large image pulls
- `--pull=always` enables self-update on restart

## Self-Update Flow

The `--network=host` + `--pull=always` + `Restart=always` combination enables self-updating:

1. Process manager (stormd) detects update signal
2. stormd exits -> container exits
3. systemd restarts the unit
4. `podman run --pull=always` pulls fresh image from registry
5. New container starts with updated binary

## Security Note

Host networking means the container shares the host's network namespace. This is acceptable because:
- These are dedicated bare metal hosts running a single purpose
- Hosts are on private networks behind MikroTik firewalls
- No multi-tenant isolation requirement
- Privileged containers already have full host access anyway

## TLS Note

The registry (`registry.gt.lo:5000`) uses TLS. For the outer `podman run --pull=always`, either:
- Configure insecure registry in `/etc/containers/registries.conf.d/` (done in ignition template)
- Or mount the CA cert into the container
