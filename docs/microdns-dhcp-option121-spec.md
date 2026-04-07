# MicroDNS DHCP Option 121 (Classless Static Routes) -- API Spec

## Background

CloudID is an Afterburn-compatible metadata service that provisions SSH keys, ignition configs, and kickstart configs to bare metal hosts during PXE boot. Hosts discover the metadata service at `169.254.169.254`, which is a link-local address that normally stays on the local segment.

To make this work without per-network router configuration, DHCP needs to push a classless static route (RFC 3442, DHCP option 121) that tells hosts to route `169.254.169.254/32` via their default gateway. The gateway (MikroTik) then DNATs the traffic to CloudID's real address.

CloudID will call this API on startup to ensure the route is configured on each data network's DHCP server.

## Requirements

### 1. New DHCP option 121 support in DHCP pools

DHCP responses for pools that have static routes configured must include option 121 (Classless Static Routes) per RFC 3442.

Option 121 encodes routes as:
```
[mask-width] [significant-octets-of-subnet] [router-ip]
```

Example: `169.254.169.254/32 via 192.168.10.1` encodes as:
```
0x20 0xa9 0xfe 0xa9 0xfe 0xc0 0xa8 0x0a 0x01
```

Per RFC 3442, when option 121 is present, the client MUST ignore option 3 (default gateway) and use only the routes from option 121. Therefore, **the default route (0.0.0.0/0 via gateway) must be included in option 121 alongside any additional static routes** so the client retains its default gateway.

### 2. REST API for managing static routes

#### List static routes for a pool

```
GET /api/v1/dhcp/pools/{pool_id}/routes
```

Response:
```json
{
    "routes": [
        {
            "id": "uuid",
            "destination": "169.254.169.254/32",
            "gateway": "192.168.10.1",
            "managed_by": "cloudid"
        }
    ]
}
```

#### Add a static route to a pool

```
POST /api/v1/dhcp/pools/{pool_id}/routes
Content-Type: application/json

{
    "destination": "169.254.169.254/32",
    "gateway": "192.168.10.1",
    "managed_by": "cloudid"
}
```

Response (201 Created):
```json
{
    "id": "uuid",
    "destination": "169.254.169.254/32",
    "gateway": "192.168.10.1",
    "managed_by": "cloudid"
}
```

Duplicate detection: if the exact `destination` + `gateway` already exists, return the existing route with `200 OK` (same pattern as DNS record dedup).

#### Delete a static route

```
DELETE /api/v1/dhcp/pools/{pool_id}/routes/{route_id}
```

Response: `204 No Content`

### 3. TOML config alternative

Static routes should also be configurable in the DHCP pool TOML config for cases where they're set manually rather than via API:

```toml
[[dhcp.v4.pools]]
range_start = "192.168.10.10"
range_end = "192.168.10.210"
subnet = "192.168.10.0/24"
gateway = "192.168.10.1"
dns = ["192.168.1.252"]
domain = "g10.lo"
lease_time_secs = 600

[[dhcp.v4.pools.static_routes]]
destination = "169.254.169.254/32"
gateway = "192.168.10.1"
```

Routes from config and API are merged. The default route (`0.0.0.0/0 via <pool gateway>`) is always implicitly included when option 121 is emitted.

### 4. Option encoding

When building the DHCP response, if a pool has any static routes (from config or API):

1. Add the default route: `0.0.0.0/0 via <pool.gateway>`
2. Add all configured static routes
3. Encode as option 121 per RFC 3442
4. Include in the DHCP OFFER and ACK responses

If no static routes are configured for a pool, omit option 121 entirely (current behavior).

## CloudID integration

On startup, CloudID will:

1. For each configured data network, `GET /api/v1/dhcp/pools` to find the pool
2. `GET /api/v1/dhcp/pools/{pool_id}/routes` to check if the metadata route exists
3. If missing, `POST /api/v1/dhcp/pools/{pool_id}/routes` with `169.254.169.254/32 via <gateway>`
4. Periodically verify the route is still present (self-healing)

The `managed_by` field lets MicroDNS and operators distinguish CloudID-managed routes from manually configured ones.

## References

- RFC 3442: The Classless Static Route Option for DHCPv4
- RFC 2132: DHCP Options (option 3 -- Router)
