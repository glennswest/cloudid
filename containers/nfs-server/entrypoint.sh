#!/bin/sh
set -e

# Load NFS kernel module
modprobe nfsd 2>/dev/null || true

# Start rpcbind
rpcbind -w || { echo "rpcbind failed"; exit 1; }

# Export filesystems from /etc/exports
exportfs -ra

# Start mountd
rpc.mountd --no-nfs-version 3

# Start NFS daemon (NFSv4 only, 8 threads)
rpc.nfsd --no-nfs-version 3 8

echo "NFS server started - exports:"
exportfs -v

# Wait for nfsd to exit
while kill -0 "$(cat /run/nfs/nfsd.pid 2>/dev/null || echo 0)" 2>/dev/null; do
    sleep 5
done

# Fallback: just stay alive
exec sleep infinity
