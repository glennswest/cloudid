# Fedora 43 Kickstart — ISO install
# CloudID template: SSH keys merged automatically
#
# Boot host from Fedora 43 ISO, pass kernel parameter:
#   inst.ks=http://192.168.200.20:8090/config/kickstart

# Install from Fedora 43 network repos
url --mirrorlist=https://mirrors.fedoraproject.org/mirrorlist?repo=fedora-43&arch=x86_64

# System config
lang en_US.UTF-8
keyboard us
timezone UTC --utc
selinux --enforcing
firewall --enabled --ssh

# Network — DHCP with hostname from CloudID
network --bootproto=dhcp --device=link --activate --hostname={{HOSTNAME}}

# Root password locked — SSH key access only
rootpw --lock

# Disk — wipe sda, simple layout
zerombr
clearpart --all --initlabel --drives=sda
autopart --type=plain

# Bootloader
bootloader --location=mbr --boot-drive=sda

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
# Harden SSH
sed -i 's/^#*PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config
sed -i 's/^#*PasswordAuthentication.*/PasswordAuthentication no/' /etc/ssh/sshd_config

# Enable SSH key access for root (keys injected by CloudID merge)
mkdir -p /root/.ssh
chmod 700 /root/.ssh
restorecon -R /root/.ssh

systemctl enable sshd
%end
