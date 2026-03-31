#!/bin/bash
set -euo pipefail

MKUBE_API="http://192.168.200.2:8082"
CDROM_NAME="fedora43"
ISO_NAME="fedora43.iso"
WORK="/data/f43build"
F43_URL="https://dl.fedoraproject.org/pub/fedora/linux/releases/43/Everything/x86_64/os/images/boot.iso"
F43_REPO="https://dl.fedoraproject.org/pub/fedora/linux/releases/43/Everything/x86_64/os/"
CLOUDID_KS="http://192.168.200.20:8090/config/kickstart"

# Install build tools
echo "=== Installing build tools ==="
dnf install -y xorriso createrepo_c dnf-plugins-core jq

mkdir -p "$WORK"

# Download Fedora 43 boot.iso
if [ ! -f "$WORK/boot.iso" ]; then
    echo "=== Downloading Fedora 43 boot.iso ==="
    rm -f "$WORK/boot.iso.partial"
    curl -L --retry 3 --retry-delay 5 -o "$WORK/boot.iso.partial" "$F43_URL"
    mv "$WORK/boot.iso.partial" "$WORK/boot.iso"
else
    echo "=== Using cached boot.iso ==="
fi

# === Download all packages for offline DVD ===
echo "=== Downloading all RPMs for offline DVD ==="
PKGDIR="$WORK/Packages"
rm -rf "$PKGDIR"
mkdir -p "$PKGDIR"

# Package list — matches the CloudID kickstart template
GROUPS="@core @standard"
PACKAGES="openssh-server openssh-clients chrony vim-enhanced tmux git rsync htop curl wget jq bash-completion podman buildah bind-utils iproute iputils"

echo "Groups: $GROUPS"
echo "Packages: $PACKAGES"

# Resolve @groups to package names
F43_REPO_OPTS="--repofrompath=f43,$F43_REPO --repo=f43 --releasever=43"
GROUP_PKGS=""
for grp in $GROUPS; do
    grpname="${grp#@}"
    echo "=== Resolving group: $grpname ==="
    resolved=$(dnf $F43_REPO_OPTS group info "$grpname" 2>/dev/null \
        | grep -E '^ ' | sed 's/^ *//' | cut -d' ' -f1 || true)
    GROUP_PKGS="$GROUP_PKGS $resolved"
done

ALL_PKGS="$GROUP_PKGS $PACKAGES"
echo "=== Total packages to download ==="
echo "$ALL_PKGS" | tr ' ' '\n' | grep -v '^$' | wc -l

# Download all packages + dependencies
dnf download --resolve --alldeps \
    --destdir="$PKGDIR" \
    --repofrompath=f43,"$F43_REPO" \
    --repo=f43 \
    --releasever=43 \
    --forcearch=x86_64 \
    --skip-unavailable \
    $ALL_PKGS

echo "=== Downloaded $(ls "$PKGDIR"/*.rpm 2>/dev/null | wc -l) RPMs ==="
du -sh "$PKGDIR"

# Create repo metadata
echo "=== Creating repository metadata ==="
createrepo_c "$PKGDIR"

# === Build the DVD ISO ===
echo "=== Building DVD ISO ==="
EXTRACT="$WORK/isoextract"
rm -rf "$EXTRACT"
mkdir -p "$EXTRACT"

# Extract boot.iso
xorriso -osirrox on -indev "$WORK/boot.iso" -extract / "$EXTRACT"
chmod -R u+w "$EXTRACT"

# Copy packages into ISO tree
cp -a "$PKGDIR" "$EXTRACT/Packages"
cp -a "$PKGDIR/repodata" "$EXTRACT/repodata"

# Get original volume ID
ORIG_VOLID=$(xorriso -indev "$WORK/boot.iso" -pvd_info 2>&1 | grep "Volume Id" | sed 's/.*: //' | tr -d "'" | xargs)
echo "Original Volume ID: $ORIG_VOLID"

# Patch every grub.cfg
for grubcfg in $(find "$EXTRACT" -name 'grub.cfg' 2>/dev/null); do
    echo "Patching $grubcfg"
    # Serial terminal
    sed -i '1i serial --unit=1 --speed=115200\nterminal_input serial console\nterminal_output serial console' "$grubcfg"
    # Auto-install: timeout 0, first entry
    sed -i 's/^set timeout=.*/set timeout=0/' "$grubcfg"
    sed -i 's/^set default=.*/set default="0"/' "$grubcfg"
    # Keep original inst.stage2=hd:LABEL but add iBFT + network + kickstart
    # rd.iscsi.firmware tells dracut to reconnect iSCSI via iBFT (detected in boot log)
    sed -i '/^\s*linux\|^\s*linuxefi/ s|$| rd.iscsi.firmware ip=dhcp inst.ks='"${CLOUDID_KS}"' earlycon=uart8250,io,0x2f8,115200n8 console=tty0 console=ttyS1,115200n8 console=ttyS0,115200n8|' "$grubcfg"
    # Remove media check and quiet
    sed -i 's/ rd.live.check//g' "$grubcfg"
    sed -i 's/ quiet//g' "$grubcfg"
    echo "--- Patched grub.cfg ---"
    cat "$grubcfg"
    echo "--- end ---"
done

# Build ISO using xorriso modify mode
echo "=== Building final ISO with xorriso (modify mode) ==="
MAP_ARGS="-map $EXTRACT/Packages /Packages"
MAP_ARGS="$MAP_ARGS -map $EXTRACT/repodata /repodata"
for grubcfg in $(find "$EXTRACT" -name 'grub.cfg' 2>/dev/null); do
    REL_PATH="${grubcfg#$EXTRACT}"
    MAP_ARGS="$MAP_ARGS -map $grubcfg $REL_PATH"
    echo "Will map: $REL_PATH"
done

xorriso -indev "$WORK/boot.iso" \
    -outdev "$WORK/$ISO_NAME" \
    $MAP_ARGS \
    -boot_image any replay \
    -volid "$ORIG_VOLID" \
    -commit

ls -lh "$WORK/$ISO_NAME"

# Push to mkube iSCSI CDROM
echo "=== Creating iSCSI CDROM: ${CDROM_NAME} ==="
curl -sf -X DELETE "$MKUBE_API/api/v1/iscsi-cdroms/$CDROM_NAME" 2>/dev/null || true
sleep 2

curl -sf -X POST "$MKUBE_API/api/v1/iscsi-cdroms" \
    -H 'Content-Type: application/json' \
    -d "{\"metadata\":{\"name\":\"$CDROM_NAME\"},\"spec\":{\"isoFile\":\"$ISO_NAME\",\"description\":\"Fedora 43 DVD + CloudID kickstart (offline)\",\"readOnly\":true}}"
echo ""

echo "=== Uploading ISO to mkube ==="
curl -f -X POST "$MKUBE_API/api/v1/iscsi-cdroms/$CDROM_NAME/upload" \
    -F "iso=@$WORK/$ISO_NAME"
echo ""

echo "=== Verifying ==="
curl -sf "$MKUBE_API/api/v1/iscsi-cdroms/$CDROM_NAME" | jq .status

echo ""
echo "=== Done! CDROM ready: ${CDROM_NAME} ==="
