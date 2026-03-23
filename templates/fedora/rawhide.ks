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
@development-tools

# SSH & system
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
strace
ltrace
perf
bpftrace

# Rust toolchain
rust
cargo
rustfmt
clippy
rust-src
rust-std-static

# Go toolchain
golang
golang-bin

# C/C++ toolchain
gcc
gcc-c++
clang
llvm
lld
cmake
meson
ninja-build
autoconf
automake
libtool
pkgconf

# Kernel development
kernel-devel
kernel-headers
kernel-modules-extra
elfutils-libelf-devel
dwarves
bc
flex
bison
openssl-devel
ncurses-devel
sparse
cscope
ctags

# Libraries (build deps)
glibc-devel
glibc-static
musl-libc
musl-gcc
liburing-devel
iscsi-initiator-utils
openssl-devel
zlib-devel
libffi-devel
sqlite-devel
bzip2-devel
xz-devel
readline-devel
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

# Install rustup for the full Rust toolchain (nightly, cross-compile targets, etc.)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
source /root/.cargo/env
rustup target add x86_64-unknown-linux-musl
rustup target add aarch64-unknown-linux-musl
rustup component add rust-src rust-analyzer

# Set GOPATH
echo 'export GOPATH=/root/go' >> /root/.bashrc
echo 'export PATH=$PATH:/root/go/bin:/root/.cargo/bin' >> /root/.bashrc
%end
