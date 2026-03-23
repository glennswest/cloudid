# Fedora Rawhide Kickstart — server2
# CloudID template: SSH keys merged automatically

# Install from Fedora Rawhide repos
url --url=https://dl.fedoraproject.org/pub/fedora/linux/development/rawhide/Everything/x86_64/os/

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
