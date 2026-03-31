#!/bin/bash
set -euo pipefail

MKUBE_API="http://192.168.200.2:8082"
CDROM_NAME="fedora43"
ISO_NAME="fedora43.iso"
WORK="/data/f43build"
F43_URL="https://dl.fedoraproject.org/pub/fedora/linux/releases/43/Everything/x86_64/os/images/boot.iso"
F43_REPO="https://dl.fedoraproject.org/pub/fedora/linux/releases/43/Everything/x86_64/os/"
# Kickstart is embedded in ISO (matching rawhidebuild pattern)
# CloudID merges SSH keys at boot time via %post timer

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

# Use dnf install --downloadonly with a temporary installroot.
# This is the only reliable way to resolve @group packages on dnf5 —
# it uses the full dependency solver with proper group expansion.
INSTALLROOT="$WORK/installroot"
rm -rf "$INSTALLROOT"

# Explicit packages from kickstart + Anaconda hardware-detected requirements
EXTRA_PKGS="openssh-server openssh-clients chrony vim-enhanced tmux git rsync htop curl wget jq bash-completion podman buildah bind-utils iproute iputils grub2 grub2-tools grub2-tools-minimal grub2-tools-extra grub2-pc shim-x64 grub2-efi-x64 efibootmgr iscsi-initiator-utils NetworkManager firewalld sudo dracut-config-rescue kernel"

echo "=== Resolving and downloading all packages via installroot ==="
dnf install -y --downloadonly \
    --installroot="$INSTALLROOT" \
    --repofrompath=f43,"$F43_REPO" \
    --repo=f43 \
    --releasever=43 \
    --setopt=keepcache=1 \
    @core @standard $EXTRA_PKGS

# Collect all downloaded RPMs from the dnf cache inside installroot
echo "=== Collecting RPMs from installroot cache ==="
find "$INSTALLROOT" -name '*.rpm' -exec cp {} "$PKGDIR/" \;

# If installroot cache was empty, check host cache as fallback
if [ "$(ls "$PKGDIR"/*.rpm 2>/dev/null | wc -l)" -eq 0 ]; then
    echo "WARNING: No RPMs in installroot cache, checking host cache"
    find /var/cache/libdnf5 /var/cache/dnf -name '*.rpm' 2>/dev/null -exec cp {} "$PKGDIR/" \;
fi

rm -rf "$INSTALLROOT"

echo "=== Downloaded $(ls "$PKGDIR"/*.rpm 2>/dev/null | wc -l) RPMs ==="
du -sh "$PKGDIR"

# Download comps.xml (group definitions) from Fedora 43 repo
echo "=== Downloading comps.xml for package group definitions ==="
REPOMD_URL="${F43_REPO}repodata/repomd.xml"
COMPS_HREF=$(curl -sf "$REPOMD_URL" | grep -oP 'href="[^"]*comps[^"]*\.xml(\.gz|\.xz|\.zst)?' | head -1 | sed 's/href="//')
if [ -n "$COMPS_HREF" ]; then
    echo "Found comps file: $COMPS_HREF"
    curl -L --retry 3 -o "$WORK/comps-raw" "${F43_REPO}${COMPS_HREF}"
    # Decompress if needed
    case "$COMPS_HREF" in
        *.gz)  gunzip -c "$WORK/comps-raw" > "$WORK/comps.xml" ;;
        *.xz)  xz -dc "$WORK/comps-raw" > "$WORK/comps.xml" ;;
        *.zst) zstd -dc "$WORK/comps-raw" > "$WORK/comps.xml" ;;
        *)     mv "$WORK/comps-raw" "$WORK/comps.xml" ;;
    esac
    echo "comps.xml size: $(wc -c < "$WORK/comps.xml") bytes"
    COMPS_ARG="-g $WORK/comps.xml"
else
    echo "WARNING: Could not find comps.xml in repo metadata"
    COMPS_ARG=""
fi

# Create repo metadata with group definitions
echo "=== Creating repository metadata ==="
createrepo_c $COMPS_ARG "$PKGDIR"

# === Build the DVD ISO ===
echo "=== Building DVD ISO ==="
EXTRACT="$WORK/isoextract"
rm -rf "$EXTRACT"
mkdir -p "$EXTRACT"

# Extract boot.iso
xorriso -osirrox on -indev "$WORK/boot.iso" -extract / "$EXTRACT"
chmod -R u+w "$EXTRACT"

# Copy kickstart and packages into ISO tree
cp /build/templates/fedora/f43.ks "$EXTRACT/ks.cfg"
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
    # rd.iscsi.firmware + ip=dhcp: dracut reconnects iSCSI target after iPXE handoff
    # inst.ks=cdrom:/ks.cfg: kickstart embedded in ISO
    sed -i '/^\s*linux\|^\s*linuxefi/ s|$| rd.iscsi.firmware ip=dhcp inst.ks=cdrom:/ks.cfg earlycon=uart8250,io,0x2f8,115200n8 console=tty0 console=ttyS1,115200n8 console=ttyS0,115200n8|' "$grubcfg"
    # Remove media check and quiet
    sed -i 's/ rd.live.check//g' "$grubcfg"
    sed -i 's/ quiet//g' "$grubcfg"
    echo "--- Patched grub.cfg ---"
    cat "$grubcfg"
    echo "--- end ---"
done

# Build ISO using xorriso modify mode
echo "=== Building final ISO with xorriso (modify mode) ==="
MAP_ARGS="-map $EXTRACT/ks.cfg /ks.cfg"
MAP_ARGS="$MAP_ARGS -map $EXTRACT/Packages /Packages"
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
