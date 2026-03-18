#!/bin/bash
# Build cloudid binary and container image locally
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

REGISTRY="registry.gt.lo:5000"
IMAGE="$REGISTRY/cloudid:edge"
STORMD_BIN="../stormd/target/aarch64-unknown-linux-musl/release/stormd"

echo "=== Building cloudid ==="

# Cross-compile binary locally for ARM64 Linux (static musl)
echo "Building binary for aarch64-unknown-linux-musl..."
cargo build --release --target aarch64-unknown-linux-musl

# Copy binaries to project root for Dockerfile
cp target/aarch64-unknown-linux-musl/release/cloudid cloudid

if [ -f "$STORMD_BIN" ]; then
    cp "$STORMD_BIN" stormd
    echo "Using stormd from $STORMD_BIN"
else
    echo "ERROR: stormd binary not found at $STORMD_BIN"
    echo "Build stormd first: cd ../stormd && cargo build --release --target aarch64-unknown-linux-musl"
    rm -f cloudid
    exit 1
fi

# Build scratch container image with podman
echo "Building container image..."
podman build --platform linux/arm64 -f Dockerfile -t "$IMAGE" .

# Clean up local binary copies
rm -f cloudid stormd

echo ""
echo "=== Build complete ==="
echo "Image: $IMAGE"
echo "Run ./deploy.sh to push and deploy"
