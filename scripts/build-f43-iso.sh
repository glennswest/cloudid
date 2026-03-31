#!/bin/bash
set -euo pipefail

MKUBE_API="http://192.168.200.2:8082"
CLOUDID="http://192.168.200.20:8090"
CDROM_NAME="fedora43"
ISO_NAME="fedora43.iso"
WORK="/data/f43build"
F43_URL="https://dl.fedoraproject.org/pub/fedora/linux/releases/43/Everything/x86_64/os/images/boot.iso"
F43_REPO="https://dl.fedoraproject.org/pub/fedora/linux/releases/43/Everything/x86_64/os/"

# Install build tools
echo "=== Installing build tools ==="
dnf install -y xorriso createrepo_c dnf-plugins-core jq

mkdir -p "$WORK"

# Download Fedora 43 boot.iso
if [ ! -f "$WORK/boot.iso" ]; then
    echo "=== Downloading Fedora 43 boot.iso ==="
    curl -L -o "$WORK/boot.iso" "$F43_URL"
else
    echo "=== Using cached boot.iso ==="
fi

# Write kickstart — uses cdrom as install source (packages on disc)
# SSH keys are fetched from CloudID at install time
echo "=== Writing kickstart ==="
cat > "$WORK/cloudid.ks" << 'KSEOF'
cdrom
lang en_US.UTF-8
keyboard us
timezone America/Chicago --utc
selinux --enforcing
firewall --enabled --ssh
network --bootproto=dhcp --device=link --activate
rootpw --lock
zerombr
clearpart --all --initlabel --drives=sda
autopart --type=plain
bootloader --location=mbr --boot-drive=sda --append="earlycon=uart8250,io,0x2f8,115200n8 console=tty0 console=ttyS1,115200n8 console=ttyS0,115200n8"
services --enabled=sshd,chronyd
reboot

%packages
@core
@standard
openssh-server
openssh-clients
chrony
vim-enhanced
tmux
git
rsync
htop
curl
wget
jq
bash-completion
podman
buildah
bind-utils
iproute
iputils
%end

%post --log=/root/ks-post.log
set -ex

# SSH config
sed -i 's/^#*PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config
sed -i 's/^#*PasswordAuthentication.*/PasswordAuthentication no/' /etc/ssh/sshd_config
mkdir -p /root/.ssh && chmod 700 /root/.ssh

# Fetch SSH keys from CloudID at install time
CLOUDID="http://192.168.200.20:8090"
for idx in $(curl -sf "${CLOUDID}/latest/meta-data/public-keys/" 2>/dev/null | grep -oP '^\d+' || true); do
    curl -sf "${CLOUDID}/latest/meta-data/public-keys/${idx}/openssh-key" >> /root/.ssh/authorized_keys 2>/dev/null || true
done
[ -f /root/.ssh/authorized_keys ] && chmod 600 /root/.ssh/authorized_keys
restorecon -R /root/.ssh 2>/dev/null || true

# CloudID SSH key refresh timer
cat > /etc/systemd/system/cloudid-keys.service << 'SVCEOF'
[Unit]
Description=Refresh SSH keys from CloudID
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart=/bin/bash -c 'curl -sf http://192.168.200.20:8090/latest/meta-data/public-keys/ 2>/dev/null | grep -oP "^\\d+" | while read idx; do curl -sf "http://192.168.200.20:8090/latest/meta-data/public-keys/$idx/openssh-key"; done > /tmp/keys.tmp && [ -s /tmp/keys.tmp ] && mv /tmp/keys.tmp /root/.ssh/authorized_keys && chmod 600 /root/.ssh/authorized_keys || rm -f /tmp/keys.tmp'
SVCEOF

cat > /etc/systemd/system/cloudid-keys.timer << 'TMREOF'
[Unit]
Description=Refresh SSH keys from CloudID every 5 minutes

[Timer]
OnBootSec=30
OnUnitActiveSec=300

[Install]
WantedBy=timers.target
TMREOF

systemctl enable cloudid-keys.timer sshd
systemctl enable serial-getty@ttyS0.service
systemctl enable serial-getty@ttyS1.service

# Configure GRUB for serial on the installed system
cat > /etc/default/grub.d/serial-console.cfg << 'GRUBEOF'
GRUB_TERMINAL="serial console"
GRUB_SERIAL_COMMAND="serial --unit=1 --speed=115200"
GRUBEOF
grub2-mkconfig -o /boot/grub2/grub.cfg 2>/dev/null || true
%end
KSEOF

# === Download all packages for offline install ===
echo "=== Downloading all RPMs for offline DVD ==="
PKGDIR="$WORK/Packages"
rm -rf "$PKGDIR"
mkdir -p "$PKGDIR"

# Extract package list from kickstart
GROUPS=""
PACKAGES=""
while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    if [[ "$line" == @* ]]; then
        GROUPS="$GROUPS $line"
    else
        PACKAGES="$PACKAGES $line"
    fi
done < <(sed -n '/%packages/,/%end/{/%packages/d;/%end/d;/^#/d;/^$/d;p}' "$WORK/cloudid.ks")

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

# Copy kickstart and packages into ISO tree
cp "$WORK/cloudid.ks" "$EXTRACT/ks.cfg"
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
    # Replace label-based stage2 with cdrom (iSCSI CDROM has no label visible to dracut)
    sed -i 's|inst.stage2=hd:LABEL=[^ ]*|inst.stage2=cdrom|g' "$grubcfg"
    # Add kickstart + console to kernel lines
    sed -i '/^\s*linux\|^\s*linuxefi/ s|$| inst.ks=cdrom:/ks.cfg earlycon=uart8250,io,0x2f8,115200n8 console=tty0 console=ttyS1,115200n8 console=ttyS0,115200n8|' "$grubcfg"
    # Remove media check
    sed -i 's/ rd.live.check//g' "$grubcfg"
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
    -d "{\"metadata\":{\"name\":\"$CDROM_NAME\"},\"spec\":{\"isoFile\":\"$ISO_NAME\",\"description\":\"Fedora 43 DVD + CloudID SSH\",\"readOnly\":true}}"
echo ""

echo "=== Uploading ISO to mkube ==="
curl -f -X POST "$MKUBE_API/api/v1/iscsi-cdroms/$CDROM_NAME/upload" \
    -F "iso=@$WORK/$ISO_NAME"
echo ""

echo "=== Verifying ==="
curl -sf "$MKUBE_API/api/v1/iscsi-cdroms/$CDROM_NAME" | jq .status

echo ""
echo "=== Done! CDROM ready: ${CDROM_NAME} ==="
echo "To boot server2:"
echo "  mk patch bmh/server2 --type=merge -p '{\"spec\":{\"image\":\"${CDROM_NAME}\"}}'"
echo "  mk annotate bmh/server2 bmh.mkube.io/reboot=\$(date -u +%Y-%m-%dT%H:%M:%SZ) --overwrite"
