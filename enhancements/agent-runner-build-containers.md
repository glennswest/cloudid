# Enhancement: agent-runner template for build container model

## Summary

The mkube-agent has been rewritten to use a **build container model**. Instead of executing inline scripts directly, the agent now uses `podman` to spawn disposable build containers (Fedora stable or rawhide) that clone a git repo, run a build script, and are disposed after completion.

The `fcos/agent-runner` ignition template needs to support this.

## Current State

The template at `templates/fcos/agent-runner.ign.json` already has:
- `/var/data` directory
- Insecure registry config for `registry.gt.lo:5000`
- Systemd service running `registry.gt.lo:5000/mkube-agent:edge` with `--privileged`, `--network host`, `--pull=always`

## Observed Issues

After rebooting server1 with `template: fcos/agent-runner`, the host came up but:
- `podman ps -a` — empty (no containers)
- `podman images` — empty (no images)
- The `mkube-agent.service` systemd unit either wasn't installed or failed to start

**Likely causes:**
1. Ignition template variable substitution may have failed (e.g. `{{HOSTNAME_ENCODED}}`)
2. Registry not reachable at early boot (DNS for `registry.gt.lo` not resolved yet)
3. `--pull=always` with no cached fallback — if the first pull fails, the service fails and never recovers despite `Restart=always` (RestartSec=5 but pull timeout is huge)

## Required Changes

### 1. Registry DNS resilience

The agent pull happens early in boot. DNS for `registry.gt.lo` may not be up yet. Add a pre-start wait loop or use the registry IP directly:

```
ExecStartPre=/bin/bash -c 'for i in $(seq 1 30); do curl -sf --connect-timeout 2 https://192.168.200.3:5000/v2/ -k && break; sleep 5; done'
```

Or add a `/etc/hosts` entry for `registry.gt.lo` in the ignition:

```json
{
  "path": "/etc/hosts",
  "mode": 420,
  "append": [{ "source": "data:,192.168.200.3%20registry.gt.lo%0A" }]
}
```

### 2. Podman-in-podman storage

The agent container (fedora:rawhide base) runs podman inside itself to spawn build containers. With `--privileged` this mostly works, but confirm:
- `fuse-overlayfs` works inside the container (already installed in agent image)
- `/dev/fuse` is accessible (covered by `--privileged`)
- Podman storage inside the container has enough space — consider mounting a host directory for container storage:

```
-v /var/lib/containers/agent-storage:/var/lib/containers
```

This prevents the agent's build container images from filling up the container's overlay filesystem.

### 3. Build image pre-pull (optional optimization)

Build containers (`fedoradev:latest`, `rawhidedev:latest`) are ~1-2GB. Pre-pulling them avoids slow first-job startup:

```
ExecStartPre=/usr/bin/podman pull --tls-verify=false registry.gt.lo:5000/rawhidedev:latest
```

### 4. Verify template variable substitution

Ensure `{{HOSTNAME_ENCODED}}` is correctly substituted by cloudid. If not, the ignition is invalid and CoreOS ignores the entire template (no systemd units installed).

## Agent Container Details

- **Image**: `registry.gt.lo:5000/mkube-agent:edge`
- **Base**: `fedora:rawhide` (has podman, fuse-overlayfs, git)
- **Process supervisor**: stormd (PID 1, manages agent process, provides SSH on port 22)
- **Ports**: 9080 (stormd API), 8080 (stormlog terminal), 22 (SSH)
- **Required flags**: `--privileged` (for podman-in-podman), `--network host`
- **Environment**: `MKUBE_API=http://192.168.200.2:8082`
- **Mounts**: `/var/data:/data` (artifact output), optionally `/var/lib/containers/agent-storage:/var/lib/containers`

## Build Container Images

| Image | Tag | Purpose |
|-------|-----|---------|
| `registry.gt.lo:5000/fedoradev` | `latest` | Stable Fedora build environment |
| `registry.gt.lo:5000/rawhidedev` | `latest` | Fedora rawhide (bleeding edge, default) |

## Job Flow

1. mkube scheduler assigns job to BMH, powers on host
2. CoreOS boots, cloudid applies `fcos/agent-runner` ignition
3. `mkube-agent.service` starts, pulls agent image, runs container
4. Agent polls `GET /api/v1/agent/work` until job is assigned
5. Agent receives job with `repo` (git URL) + `buildScript` (script name) + `buildImage`
6. Agent runs: `podman pull <buildImage>` then `podman run --rm <buildImage> bash -c "git clone <repo> /build && cd /build && ./<buildScript>"`
7. Logs streamed to mkube, exit code reported, build container disposed
8. Agent exits, stormd restarts it, polls for next job
