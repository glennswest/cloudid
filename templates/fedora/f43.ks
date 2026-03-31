# Fedora 43 Kickstart — ISO install
# CloudID template: SSH keys merged automatically
#
# iSCSI CDROM boot: iPXE SAN boots from iSCSI target, kernel reuses connection
# Kernel params: rd.iscsi.firmware ip=ibft inst.ks=cdrom:/ks.cfg

# Install source set via kernel param: inst.repo=hd:LABEL=<volid>

# System config
lang en_US.UTF-8
keyboard us
timezone UTC --utc
selinux --enforcing
firewall --enabled --ssh

# Network — single NIC for installer, bonding configured in %post for installed system
network --bootproto=dhcp --device=enp3s0 --activate

# Root password locked — SSH key access only
rootpw --lock

# Disk — wipe sda, simple layout
zerombr
clearpart --all --initlabel --drives=sda
autopart --type=plain

# Bootloader
bootloader --location=mbr --boot-drive=sda --append="earlycon=uart8250,io,0x2f8,115200n8 console=tty0 console=ttyS1,115200n8 console=ttyS0,115200n8"

# Services
services --enabled=sshd,chronyd

# Reboot after install
reboot

# Text mode for serial console
text

%packages --ignoremissing
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
# Anaconda requires these based on hardware/config (not in @core/@standard)
grub2
grub2-tools
grub2-tools-minimal
grub2-tools-extra
grub2-pc
shim-x64
grub2-efi-x64
efibootmgr
iscsi-initiator-utils
NetworkManager
firewalld
sudo
dracut-config-rescue
%end

%post --log=/root/ks-post.log
set -ex

# Load bonding kernel module at boot
echo bonding > /etc/modules-load.d/bonding.conf

# LACP bonding (802.3ad) across both NICs — applied to installed system
cat > /etc/NetworkManager/system-connections/bond0.nmconnection << 'BONDEOF'
[connection]
id=bond0
type=bond
interface-name=bond0

[bond]
mode=802.3ad
miimon=100
lacp_rate=fast
xmit_hash_policy=layer3+4

[ipv4]
method=auto

[ipv6]
method=auto
BONDEOF

cat > /etc/NetworkManager/system-connections/bond0-port-enp3s0.nmconnection << 'PORT1EOF'
[connection]
id=bond0-port-enp3s0
type=ethernet
interface-name=enp3s0
master=bond0
slave-type=bond
PORT1EOF

cat > /etc/NetworkManager/system-connections/bond0-port-enp5s0.nmconnection << 'PORT2EOF'
[connection]
id=bond0-port-enp5s0
type=ethernet
interface-name=enp5s0
master=bond0
slave-type=bond
PORT2EOF

chmod 600 /etc/NetworkManager/system-connections/*.nmconnection

# Remove installer-created single-NIC profile so bond takes over
rm -f /etc/NetworkManager/system-connections/enp3s0.nmconnection 2>/dev/null || true

# Harden SSH
sed -i 's/^#*PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config
sed -i 's/^#*PasswordAuthentication.*/PasswordAuthentication no/' /etc/ssh/sshd_config

# Fetch SSH keys from CloudID at install time
mkdir -p /root/.ssh && chmod 700 /root/.ssh
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
mkdir -p /etc/default/grub.d
cat > /etc/default/grub.d/serial-console.cfg << 'GRUBEOF'
GRUB_TERMINAL="serial console"
GRUB_SERIAL_COMMAND="serial --unit=1 --speed=115200"
GRUBEOF
grub2-mkconfig -o /boot/grub2/grub.cfg 2>/dev/null || true
%end
