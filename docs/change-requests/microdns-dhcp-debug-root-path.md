# Change Request: MicroDNS DHCP root_path Debug & Fix

## Problem

Server2 (MAC `ac:1f:6b:8b:11:5d`, network g10) is stuck in a PXE boot loop. iPXE shows "Booting from iSCSI" and fails with error `0x3f122003` (iSCSI login failed), indicating it received a `root_path` DHCP option pointing at the baremetalservices iSCSI target (`iscsi:192.168.10.1::::iqn.2000-02.com.mikrotik:file1`).

The DHCP reservation for this MAC has:
- `root_path: ""` (empty string — intended to suppress pool default)
- `ipxe_boot_url: "http://192.168.200.2:8082/api/v1/ipxe/boot"`

The g10 pool default has:
- `root_path: "iscsi:192.168.10.1::::iqn.2000-02.com.mikrotik:file1"`

**Expected behavior:** When a reservation has `root_path: ""`, the pool default root_path should be suppressed. iPXE clients should receive `ipxe_boot_url` as the boot file and NO root_path option 17.

**Actual behavior:** iPXE is still receiving root_path and auto-sanbooting from iSCSI instead of chaining to the `ipxe_boot_url`.

## Context

mkube sets `root_path: ""` and `ipxe_boot_url` on DHCP reservations for hosts that should boot via a dynamic iPXE script endpoint (boot-once mechanism for ISO installs). The `ipxe_boot_url` only works when root_path is absent — if root_path is present, iPXE ignores the boot file URL and sanboots from iSCSI.

The pool default `root_path` for baremetalservices must remain — it's needed for unknown host discovery.

## Diagnosis Needed

The code in `crates/microdns-dhcp/src/v4/server.rs` lines 853-857 looks correct:

```rust
let effective_root_path = db_res
    .as_ref()
    .and_then(|r| r.root_path.clone())        // Should be Some("") for this reservation
    .or_else(|| pool_pxe.as_ref().and_then(|p| p.root_path.clone()))  // Should NOT run
    .filter(|rp| !rp.is_empty());              // Should filter "" to None
```

But something is wrong. Please add debug logging to determine:

1. Is the reservation found for MAC `ac:1f:6b:8b:11:5d`? (`db_res.is_some()`)
2. What is the reservation's `root_path` value? (`db_res.root_path`)
3. What is the pool's `root_path` value? (`pool_pxe.root_path`)
4. What is the final `effective_root_path`?
5. Is iPXE detection working? (`is_ipxe` value, option 175 / user-class presence)
6. What boot file is served? (the URL or regular `undionly.kpxe`)

### Suggested debug log (around line 857):

```rust
info!("DHCP DEBUG {}: db_res={} res_root={:?} pool_root={:?} effective_root={:?}",
      mac, db_res.is_some(),
      db_res.as_ref().and_then(|r| r.root_path.clone()),
      pool_pxe.as_ref().and_then(|p| p.root_path.clone()),
      effective_root_path);
```

And around line 891 (iPXE boot file selection):

```rust
info!("DHCP DEBUG {}: is_ipxe={} ipxe_url={:?} boot_file={}",
      mac, is_ipxe, effective_ipxe_url, boot_file);
```

## Possible Root Causes

1. **Empty string lost in storage round-trip** — `root_path: ""` stored as `Some("")` via REST API but read back as `None` from the database, causing pool default fallback
2. **Reservation not matched** — MAC lookup fails for some reason, so `db_res = None` and pool defaults apply everywhere
3. **iPXE not detected** — `is_ipxe` is false because the client doesn't send option 175 or user-class "iPXE", so regular boot file is served instead of `ipxe_boot_url`
4. **External DHCP source** — another DHCP responder on g10 answering before microdns (unlikely per infra owner)

## Test Plan

1. Deploy microdns with debug logging
2. Power on server2 (`mk annotate bmh/server2 bmh.mkube.io/reboot="$(date -u +%Y-%m-%dT%H:%M:%SZ)" --overwrite`)
3. Check logs: `curl -s "http://192.168.10.252:8080/api/v1/logs?limit=50" | python3 -m json.tool`
4. Power off server2 (`mk patch bmh/server2 --type=merge -p '{"spec":{"online":false}}'`)
5. Analyze the debug output to identify root cause
6. Fix and redeploy

## Affected Components

- `crates/microdns-dhcp/src/v4/server.rs` — DHCP response builder (lines 830-927)
- `crates/microdns-core/src/db.rs` — reservation storage/retrieval
- `crates/microdns-core/src/types.rs` — `DhcpDbReservation.root_path: Option<String>`

## Priority

High — blocks bare metal Fedora 43 installs via iSCSI CDROM boot.
