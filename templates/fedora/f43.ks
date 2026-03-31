# Fedora 43 Kickstart — ISO install
# CloudID template: SSH keys merged automatically
#
# Boot host from Fedora 43 ISO, pass kernel parameter:
#   inst.ks=http://192.168.200.20:8090/config/kickstart

# Install from ISO media (packages embedded in DVD)
cdrom

# System config
lang en_US.UTF-8
keyboard us
timezone UTC --utc
selinux --enforcing
firewall --enabled --ssh

# Network — LACP bonding (802.3ad) across both NICs, DHCP on bond0
network --bondslaves=enp3s0,enp5s0 --bondopts=mode=802.3ad,miimon=100,lacp_rate=fast,xmit_hash_policy=layer3+4 --bootproto=dhcp --device=bond0 --activate --hostname={{HOSTNAME}}

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
%end

%post --log=/root/ks-post.log
set -ex

# Load bonding kernel module at boot
echo bonding > /etc/modules-load.d/bonding.conf

# Harden SSH
sed -i 's/^#*PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config
sed -i 's/^#*PasswordAuthentication.*/PasswordAuthentication no/' /etc/ssh/sshd_config

# Enable SSH key access for root (keys injected by CloudID merge)
mkdir -p /root/.ssh && chmod 700 /root/.ssh
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
