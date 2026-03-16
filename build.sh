#!/bin/bash
# Build cloudid binary and container image locally
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

REGISTRY="registry.gt.lo:5000"
IMAGE="$REGISTRY/cloudid:edge"

echo "=== Building cloudid ==="

# Cross-compile binary locally for ARM64 Linux (static musl)
echo "Building binary for aarch64-unknown-linux-musl..."
cargo build --release --target aarch64-unknown-linux-musl

# Copy binary to project root for Dockerfile
cp target/aarch64-unknown-linux-musl/release/cloudid cloudid

# Build scratch container image with podman
echo "Building container image..."
podman build --platform linux/arm64 -f Dockerfile -t "$IMAGE" .

# Clean up local binary copy
rm -f cloudid

echo ""
echo "=== Build complete ==="
echo "Image: $IMAGE"
echo "Run ./deploy.sh to push and deploy"
